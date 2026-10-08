use super::*;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

pub struct Runtime {
    executable: PathBuf,
    script: Option<PathBuf>,
}
impl Runtime {
    pub fn discover(resources: &Path) -> Result<Self, AppError> {
        let name = if cfg!(windows) {
            "book-tts.exe"
        } else {
            "book-tts"
        };
        let mut roots = vec![resources.join("tts-runtime")];
        if let Ok(executable) = std::env::current_exe() {
            if let Some(parent) = executable.parent() {
                roots.push(parent.join("tts-runtime"));
            }
        }
        if cfg!(debug_assertions) {
            roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("tts-runtime"));
        }
        for root in roots {
            let executable = root.join(name);
            if executable.is_file() {
                let manifest: serde_json::Value = read_json(&root.join("manifest.json"))?;
                if manifest["version"] != 1
                    || manifest["platform"] != std::env::consts::OS
                    || manifest["arch"] != std::env::consts::ARCH
                {
                    return Err(failure("audioRuntime"));
                }
                let mut file = std::fs::File::open(&executable).map_err(io_error)?;
                let mut hash = Sha256::new();
                std::io::copy(&mut file, &mut hash).map_err(io_error)?;
                if manifest["sha256"].as_str() != Some(format!("{:x}", hash.finalize()).as_str()) {
                    return Err(failure("audioRuntime"));
                }
                return Ok(Self {
                    executable,
                    script: None,
                });
            }
        }
        if cfg!(debug_assertions) {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let executable = root.join(if cfg!(windows) {
                ".cache/tts-venv/Scripts/python.exe"
            } else {
                ".cache/tts-venv/bin/python"
            });
            let script = root.join("scripts/tts/worker.py");
            if executable.is_file()
                && root.join(".cache/tts-venv/ready.json").is_file()
                && script.is_file()
            {
                return Ok(Self {
                    executable,
                    script: Some(script),
                });
            }
        }
        Err(failure("audioRuntime"))
    }
    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        if let Some(script) = &self.script {
            command.arg("-I").arg(script);
        }
        command
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
    }
}
#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Progress {
    Chunk { completed: u32, chapter: String },
    Chapter { completed: u32 },
    Error { reason: String },
    Done,
}

pub(super) async fn run(
    lease: &ProjectLease,
    models: Arc<ModelManager>,
    runtime: Runtime,
    directory: &Path,
    view: &mut AudioJobView,
    mut cancel: watch::Receiver<bool>,
) -> Result<bool, AppError> {
    if *cancel.borrow() || lease.cancelled() {
        return Ok(false);
    }
    models
        .materialize(&directory.join("model"))
        .await
        .map_err(|_| failure("audioModels"))?;
    write_json(
        &directory.join("model-files.json"),
        &models
            .list()
            .await
            .map_err(|_| failure("audioModels"))?
            .into_iter()
            .map(|v| v.model)
            .collect::<Vec<_>>(),
    )?;
    if *cancel.borrow() || lease.cancelled() {
        return Ok(false);
    }
    let mut child = runtime
        .command()
        .arg("--parent-pipe")
        .arg("--run")
        .arg(directory)
        .spawn()
        .map_err(|_| failure("audioRuntime"))?;
    let mut lines =
        BufReader::new(child.stdout.take().ok_or_else(|| failure("audioRuntime"))?).lines();
    let mut done = false;
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
    loop {
        tokio::select! {
            biased;
            _ = cancel.changed() => { let _ = child.kill().await; return Ok(false); }
            _ = interval.tick() => { if lease.cancelled() { let _ = child.kill().await; return Ok(false); } }
            line = lines.next_line() => {
                let Some(line) = line.map_err(io_error)? else { break; };
                if line.len() > 8192 { return Err(failure("audioWorker")); }
                let progress: Progress = serde_json::from_str(&line).map_err(|_| failure("audioWorker"))?;
                match progress {
                    Progress::Chunk { completed, chapter } => {
                        if completed > view.total_chunks { return Err(failure("audioWorker")); }
                        view.completed_chunks = completed; view.current_chapter = chapter;
                    }
                    Progress::Chapter { completed } => {
                        if completed > view.total_chapters { return Err(failure("audioWorker")); }
                        view.completed_chapters = completed;
                    }
                    Progress::Done => done = true,
                    Progress::Error { reason } => {
                        let allowed = ["audioCuda", "audioModels", "audioTooLong", "audioOutput", "audioMemory", "audioStorage", "audioVersion"];
                        return Err(failure(if allowed.contains(&reason.as_str()) { &reason } else { "audioWorker" }));
                    }
                }
                write_json(&directory.join("status.json"), view)?;
            }
        }
    }
    let status = tokio::select! {
        _ = cancel.changed() => { let _ = child.kill().await; return Ok(false); }
        status = child.wait() => status.map_err(io_error)?,
    };
    if *cancel.borrow() || lease.cancelled() {
        return Ok(false);
    }
    if !status.success()
        || !done
        || view.completed_chapters != view.total_chapters
        || view.completed_chunks != view.total_chunks
    {
        return Err(failure("audioWorker"));
    }
    // Model hard links are temporary. Finished jobs keep only MP3s and snapshots.
    let _ = std::fs::remove_dir_all(directory.join("model"));
    Ok(true)
}
