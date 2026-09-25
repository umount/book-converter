//! One native process at a time, with bounded IPC and kill-on-drop cancellation.
use crate::{
    protocol::{Request, Response, MAX_REQUEST_BYTES},
    Error, Result,
};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Semaphore,
};

struct NativeWorker {
    child: Option<tokio::process::Child>,
    permit: Option<tokio::sync::SemaphorePermit<'static>>,
}
impl Drop for NativeWorker {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let permit = self.permit.take();
            // Hold admission until the killed process is reaped, including when
            // the caller aborts its future during native inference.
            tokio::spawn(async move {
                let _ = child.wait().await;
                drop(permit);
            });
        }
    }
}

static WORKERS: Semaphore = Semaphore::const_new(1);
const MAX_RESPONSE: u64 = 128 * 1024;

/// Dropping this future cancels a queued worker or kills its running process.
/// Caller owns a unique temporary output directory and discards it on cancellation;
/// only a successful, revision-checked result may be published to project storage.
pub async fn execute(executable: &Path, request: &Request, deadline: Duration) -> Result<Response> {
    request.validate()?;
    if !executable.is_absolute()
        || !std::fs::symlink_metadata(executable)?.is_file()
        || deadline.is_zero()
        || deadline > Duration::from_secs(300)
    {
        return Err(Error::Request);
    }
    let bytes = serde_json::to_vec(request).map_err(|_| Error::Request)?;
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err(Error::Request);
    }
    let permit = WORKERS.acquire().await.map_err(|_| Error::Worker)?;
    let mut command = Command::new(executable);
    // Avoid a console window flashing for each processed page on Windows.
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let child = command.arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn().map_err(|error| {
            tracing::error!(path = %executable.display(), %error, "Cannot spawn manga worker");
            error
        })?;
    let mut guard = NativeWorker {
        child: Some(child),
        permit: Some(permit),
    };
    let child = guard.child.as_mut().ok_or(Error::Worker)?;
    let mut stdin = child.stdin.take().ok_or(Error::Worker)?;
    let stdout = child.stdout.take().ok_or(Error::Worker)?;
    let operation = async {
        stdin.write_all(&bytes).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let mut response = Vec::new();
        stdout
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut response)
            .await?;
        if response.len() as u64 > MAX_RESPONSE {
            return Err(Error::Output);
        }
        let status = child.wait().await?;
        if !status.success() {
            tracing::error!(exit_code = ?status.code(), "Manga worker process exited unsuccessfully");
            let failure: serde_json::Value = serde_json::from_slice(&response).unwrap_or_default();
            return Err(match failure.get("error").and_then(|v| v.as_str()) {
                Some("text_overflow") => Error::TextOverflow,
                Some("font_coverage") => Error::FontCoverage,
                _ => Error::Worker,
            });
        }
        let response: Response = serde_json::from_slice(&response).map_err(|_| Error::Output)?;
        if response.version != 1 {
            return Err(Error::Output);
        }
        crate::validate_dimensions(response.width, response.height)?;
        Ok(response)
    };
    match tokio::time::timeout(deadline, operation).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            Err(Error::Timeout)
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::protocol::{Asset, Operation};
    use std::{
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        root: std::path::PathBuf,
    }
    impl Fixture {
        fn new(script: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "manga-worker-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let worker = root.join("worker");
            std::fs::write(&worker, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self { root }
        }
        fn request(&self) -> Request {
            Request {
                version: 1,
                runtime: self.root.join("runtime"),
                model: self.root.join("model"),
                input: Asset {
                    path: self.root.join("input"),
                    sha256: "0".repeat(64),
                },
                output: self.root.join("output"),
                operation: Operation::Masks {
                    rectangles:vec![],
                    regions: vec![],
                    margin: 2,
                },
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    async fn assert_dead(pid: &str) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while Path::new(&format!("/proc/{}", pid.trim())).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("worker must be killed and reaped within one second");
    }
    #[tokio::test]
    async fn protocol_success_nonzero_and_oversized_output() {
        for (script,success) in [
            ("cat >/dev/null\nprintf '%s' '{\"version\":1,\"width\":4,\"height\":3,\"load_millis\":0,\"inference_millis\":0}'",true),
            ("cat >/dev/null\nexit 4",false),
            ("cat >/dev/null\nprintf '%140000s' x",false),
            ("cat >/dev/null\nprintf '%s' '{\"version\":2,\"width\":4,\"height\":3,\"load_millis\":0,\"inference_millis\":0}'",false),
        ] {
            let fixture=Fixture::new(script);
            assert_eq!(execute(&fixture.root.join("worker"),&fixture.request(),Duration::from_secs(3)).await.is_ok(),success);
        }
    }
    #[tokio::test]
    async fn timeout_kills_and_reaps_native_process() {
        let fixture = Fixture::new("cat >/dev/null\necho $$ > \"$0.pid\"\nexec /bin/sleep 30");
        let result = execute(
            &fixture.root.join("worker"),
            &fixture.request(),
            Duration::from_millis(200),
        )
        .await;
        assert!(matches!(result, Err(Error::Timeout)));
        let pid = std::fs::read_to_string(fixture.root.join("worker.pid")).unwrap();
        assert_dead(&pid).await;
    }
    #[tokio::test]
    async fn dropping_running_future_kills_worker_and_releases_slot() {
        let fixture = Fixture::new("cat >/dev/null\necho $$ > \"$0.pid\"\nexec /bin/sleep 30");
        let executable = fixture.root.join("worker");
        let request = fixture.request();
        let task =
            tokio::spawn(
                async move { execute(&executable, &request, Duration::from_secs(30)).await },
            );
        let pid_path = fixture.root.join("worker.pid");
        tokio::time::timeout(Duration::from_secs(5), async {
            while !pid_path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let pid = std::fs::read_to_string(pid_path).unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_dead(&pid).await;
        let next = Fixture::new("cat >/dev/null\nexit 1");
        assert!(matches!(
            execute(
                &next.root.join("worker"),
                &next.request(),
                Duration::from_secs(1)
            )
            .await,
            Err(Error::Worker)
        ));
    }
}
