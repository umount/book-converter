use super::*;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

pub struct Runtime {
    executable: PathBuf,
    script: Option<PathBuf>,
}
impl Runtime {
    #[cfg(all(test, unix))]
    pub(super) fn for_test(executable: PathBuf) -> Self {
        Self {
            executable,
            script: None,
        }
    }
    pub fn discover(resources: &Path) -> Result<Self, AppError> {
        Self::locate(resources, true)
    }
    /// Poll availability cheaply; every job still verifies the executable at admission.
    pub(crate) fn is_available(resources: &Path) -> bool {
        Self::locate(resources, false).is_ok()
    }
    fn locate(resources: &Path, verify_contents: bool) -> Result<Self, AppError> {
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
                if verify_contents {
                    let mut file = std::fs::File::open(&executable).map_err(io_error)?;
                    let mut hash = Sha256::new();
                    std::io::copy(&mut file, &mut hash).map_err(io_error)?;
                    if manifest["sha256"].as_str()
                        != Some(format!("{:x}", hash.finalize()).as_str())
                    {
                        return Err(failure("audioRuntime"));
                    }
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
    pub(super) async fn preview(
        &self,
        lease: &ProjectLease,
        directory: &Path,
        output: &Path,
    ) -> Result<(), AppError> {
        let mut child = self
            .command()
            .arg("--parent-pipe")
            .arg("--preview")
            .arg(directory)
            .arg("--output")
            .arg(output)
            .spawn()
            .map_err(|_| failure("audioRuntime"))?;
        // Keep the parent pipe open while wait_with_output owns the child.
        let _input = child.stdin.take();
        let wait = child.wait_with_output();
        tokio::pin!(wait);
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        let result = loop {
            tokio::select! {
                result = &mut wait => break result.map_err(io_error)?,
                _ = interval.tick() => {
                    if lease.cancelled() { return Err(failure("audioPreviewUnavailable")); }
                }
            }
        };
        for line in result.stdout.split(|b| *b == b'\n') {
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(line) {
                if event["event"] == "error" {
                    return Err(worker_error(
                        event["reason"].as_str().unwrap_or("audioWorker"),
                    ));
                }
                if event["event"] == "preview" && result.status.success() {
                    return Ok(());
                }
            }
        }
        Err(failure("audioWorker"))
    }
}
#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Progress {
    Loaded { device: AudioDevice },
    Chunk { completed: u32, chapter: String },
    Chapter { completed: u32 },
    Error { reason: String },
    Done,
    Paused,
}

fn worker_error(reason: &str) -> AppError {
    let allowed = [
        "audioCuda",
        "audioModels",
        "audioTooLong",
        "audioOutput",
        "audioMemory",
        "audioStorage",
        "audioVersion",
        "audioPreviewUnavailable",
    ];
    failure(if allowed.contains(&reason) {
        reason
    } else {
        "audioWorker"
    })
}

struct SessionDirectory(PathBuf);
impl Drop for SessionDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Session {
    child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    device: AudioDevice,
    specs: Vec<crate::models::ModelSpec>,
    _directory: SessionDirectory,
}
impl Session {
    async fn spawn(
        root: &Path,
        models: Arc<ModelManager>,
        runtime: Runtime,
        device: AudioDevice,
    ) -> Result<Self, AppError> {
        let directory = SessionDirectory(root.join(uuid::Uuid::new_v4().to_string()));
        std::fs::create_dir_all(&directory.0).map_err(io_error)?;
        models
            .materialize(&directory.0.join("model"))
            .await
            .map_err(|_| failure("audioModels"))?;
        let specs = models
            .list()
            .await
            .map_err(|_| failure("audioModels"))?
            .into_iter()
            .map(|v| v.model)
            .collect::<Vec<_>>();
        write_json(&directory.0.join("model-files.json"), &specs)?;
        let mut child = runtime
            .command()
            .arg("--serve")
            .arg(&directory.0)
            .arg("--device")
            .arg(match device {
                AudioDevice::Auto => "auto",
                AudioDevice::Cpu => "cpu",
                AudioDevice::Cuda => "cuda",
            })
            .spawn()
            .map_err(|_| failure("audioRuntime"))?;
        let input = child.stdin.take().ok_or_else(|| failure("audioRuntime"))?;
        let output =
            BufReader::new(child.stdout.take().ok_or_else(|| failure("audioRuntime"))?).lines();
        let mut session = Self {
            child,
            input,
            output,
            device,
            specs,
            _directory: directory,
        };
        match session.receive().await? {
            Progress::Loaded { device } if device != AudioDevice::Auto => session.device = device,
            Progress::Error { reason } => return Err(worker_error(&reason)),
            _ => return Err(failure("audioWorker")),
        }
        Ok(session)
    }
    async fn send(&mut self, value: serde_json::Value) -> Result<(), AppError> {
        let mut bytes = serde_json::to_vec(&value).map_err(io_error)?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| failure("audioWorker"))
    }
    async fn receive(&mut self) -> Result<Progress, AppError> {
        let line = self
            .output
            .next_line()
            .await
            .map_err(|_| failure("audioWorker"))?
            .ok_or_else(|| failure("audioWorker"))?;
        if line.len() > 8192 {
            return Err(failure("audioWorker"));
        }
        serde_json::from_str(&line).map_err(|_| failure("audioWorker"))
    }
    fn compatible(&mut self, device: &AudioDevice) -> bool {
        matches!(self.child.try_wait(), Ok(None))
            && (*device == AudioDevice::Auto || self.device == *device)
    }
}

pub(super) struct Engine {
    root: PathBuf,
    session: Arc<tokio::sync::Mutex<Option<Session>>>,
    status: Mutex<AudioEngineView>,
}
impl Engine {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            session: Arc::new(tokio::sync::Mutex::new(None)),
            status: Mutex::new(AudioEngineView {
                state: AudioEngineState::Unloaded,
                device: None,
                error: None,
            }),
        }
    }
    fn set(&self, state: AudioEngineState, device: Option<AudioDevice>, error: Option<AppError>) {
        *self.status.lock().unwrap() = AudioEngineView {
            state,
            device,
            error,
        };
    }
    pub fn view(&self) -> AudioEngineView {
        if let Ok(mut slot) = self.session.try_lock() {
            if slot
                .as_mut()
                .is_some_and(|session| !matches!(session.child.try_wait(), Ok(None)))
            {
                slot.take();
                self.set(AudioEngineState::Failed, None, Some(failure("audioWorker")));
            }
        }
        self.status.lock().unwrap().clone()
    }
    async fn discard(slot: &mut Option<Session>) {
        if let Some(mut session) = slot.take() {
            let _ = session.child.kill().await;
        }
    }
    async fn ensure(
        &self,
        slot: &mut Option<Session>,
        models: Arc<ModelManager>,
        runtime: Runtime,
        device: AudioDevice,
    ) -> Result<(), AppError> {
        if slot
            .as_mut()
            .is_some_and(|session| session.compatible(&device))
        {
            return Ok(());
        }
        Self::discard(slot).await;
        self.set(AudioEngineState::Loading, Some(device.clone()), None);
        match Session::spawn(&self.root, models, runtime, device).await {
            Ok(session) => {
                self.set(AudioEngineState::Ready, Some(session.device.clone()), None);
                *slot = Some(session);
                Ok(())
            }
            Err(error) => {
                self.set(AudioEngineState::Failed, None, Some(error.clone()));
                Err(error)
            }
        }
    }
    pub fn load(
        self: &Arc<Self>,
        models: Arc<ModelManager>,
        runtime: Runtime,
        device: AudioDevice,
    ) -> Result<(), AppError> {
        let mut slot = self
            .session
            .clone()
            .try_lock_owned()
            .map_err(|_| failure("audioBusy"))?;
        if slot
            .as_mut()
            .is_some_and(|session| session.compatible(&device))
        {
            return Ok(());
        }
        self.set(AudioEngineState::Loading, Some(device.clone()), None);
        let engine = self.clone();
        tokio::spawn(async move {
            let _ = engine.ensure(&mut slot, models, runtime, device).await;
        });
        Ok(())
    }
    pub async fn unload(&self) -> Result<(), AppError> {
        let mut slot = self.session.try_lock().map_err(|_| failure("audioBusy"))?;
        Self::discard(&mut slot).await;
        self.set(AudioEngineState::Unloaded, None, None);
        Ok(())
    }
}

pub(super) async fn run(
    engine: &Engine,
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
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
    let mut slot = loop {
        tokio::select! {
            slot = engine.session.lock() => break slot,
            _ = cancel.changed() => return Ok(false),
            _ = interval.tick() => { if lease.cancelled() { return Ok(false); } }
        }
    };
    let loaded = {
        let loading = engine.ensure(&mut slot, models, runtime, view.device.clone());
        tokio::pin!(loading);
        let mut pausing = false;
        loop {
            tokio::select! {
                result = &mut loading => break Some(result),
                _ = interval.tick() => { if lease.cancelled() { break None; } }
                _ = cancel.changed(), if !pausing => {
                    pausing = true;
                    view.state = AudioState::Pausing;
                    if let Err(error) = write_json(&directory.join("status.json"), view) { break Some(Err(error)); }
                }
            }
        }
    };
    let Some(loaded) = loaded else {
        Engine::discard(&mut slot).await;
        engine.set(AudioEngineState::Unloaded, None, None);
        return Ok(false);
    };
    if let Err(error) = loaded {
        Engine::discard(&mut slot).await;
        engine.set(AudioEngineState::Failed, None, Some(error.clone()));
        return Err(error);
    }
    if lease.cancelled() {
        Engine::discard(&mut slot).await;
        engine.set(AudioEngineState::Unloaded, None, None);
        return Ok(false);
    }
    if *cancel.borrow() {
        return Ok(false);
    }
    let result = execute(
        slot.as_mut().ok_or_else(|| failure("audioRuntime"))?,
        lease,
        directory,
        view,
        cancel,
    )
    .await;
    if lease.cancelled() {
        Engine::discard(&mut slot).await;
        engine.set(AudioEngineState::Unloaded, None, None);
    } else if let Err(error) = &result {
        Engine::discard(&mut slot).await;
        engine.set(AudioEngineState::Failed, None, Some(error.clone()));
    }
    if result.is_ok() {
        // Remove model links left by jobs from the former per-job runtime.
        let _ = std::fs::remove_dir_all(directory.join("model"));
    }
    result
}

async fn execute(
    session: &mut Session,
    lease: &ProjectLease,
    directory: &Path,
    view: &mut AudioJobView,
    mut cancel: watch::Receiver<bool>,
) -> Result<bool, AppError> {
    write_json(&directory.join("model-files.json"), &session.specs)?;
    session
        .send(serde_json::json!({"command": "run", "id": view.id, "directory": directory}))
        .await?;
    let mut pausing = false;
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
    loop {
        tokio::select! {
            biased;
            _ = cancel.changed(), if !pausing => {
                pausing = true;
                view.state = AudioState::Pausing;
                write_json(&directory.join("status.json"), view)?;
                session.send(serde_json::json!({"command": "pause", "id": view.id})).await?;
            }
            _ = interval.tick() => { if lease.cancelled() { return Ok(false); } }
            progress = session.receive() => {
                match progress? {
                    Progress::Chunk { completed, chapter } => {
                        if completed > view.total_chunks { return Err(failure("audioWorker")); }
                        view.completed_chunks = completed; view.current_chapter = chapter;
                    }
                    Progress::Chapter { completed } => {
                        if completed > view.total_chapters { return Err(failure("audioWorker")); }
                        view.completed_chapters = completed;
                    }
                    Progress::Done => {
                        if view.completed_chapters != view.total_chapters || view.completed_chunks != view.total_chunks { return Err(failure("audioWorker")); }
                        return Ok(true);
                    }
                    Progress::Paused if pausing => return Ok(false),
                    Progress::Error { reason } => return Err(worker_error(&reason)),
                    _ => return Err(failure("audioWorker")),
                }
                write_json(&directory.join("status.json"), view)?;
            }
        }
    }
}
