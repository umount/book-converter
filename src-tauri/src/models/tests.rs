use super::*;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn fixture() -> (Arc<ModelManager>, ModelSpec) {
    let root = std::env::temp_dir().join(format!("model-download-{}", uuid::Uuid::new_v4()));
    let mut model = catalog::catalog().remove(0);
    model.bytes = 6;
    model.sha256 = format!("{:x}", Sha256::digest(b"abcdef"));
    (
        Arc::new(ModelManager::with_catalog(root, vec![model.clone()])),
        model,
    )
}
async fn server(
    response: &'static str,
    expected_range: &'static str,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 8192];
        let count = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..count]).to_lowercase();
        assert!(request.contains(expected_range), "{request}");
        assert!(!request.contains("authorization:"));
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    (url, task)
}
fn partial(manager: &ModelManager, model: &ModelSpec, bytes: &[u8]) -> PathBuf {
    let dir = manager.root.join(model.directory());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("artifact.part"), bytes).unwrap();
    dir
}
#[tokio::test]
async fn resumes_exact_range_then_publishes_only_verified_content() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"abc");
    assert_eq!(manager.list().await.unwrap()[0].status, ModelStatus::Paused);
    let (url, server) = server("HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef", "range: bytes=3-").await;
    let (_sender, receiver) = watch::channel(false);
    assert!(transfer::download(manager.clone(), &model, receiver, url)
        .await
        .unwrap());
    server.await.unwrap();
    assert_eq!(std::fs::read(dir.join("artifact")).unwrap(), b"abcdef");
    assert!(!dir.join("artifact.part").exists());
    let fresh = Arc::new(ModelManager::with_catalog(
        manager.root.clone(),
        vec![model],
    ));
    assert_eq!(
        fresh.list().await.unwrap()[0].status,
        ModelStatus::Downloaded
    );
    fresh.remove("qwen3-tts-06-0").await.unwrap();
    assert_eq!(fresh.list().await.unwrap()[0].status, ModelStatus::Missing);
    std::fs::remove_dir_all(&manager.root).unwrap();
}
#[tokio::test]
async fn range_ignored_restarts_instead_of_appending() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"abc");
    let (url, server) = server(
        "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef",
        "range: bytes=3-",
    )
    .await;
    let (_sender, receiver) = watch::channel(false);
    assert!(transfer::download(manager.clone(), &model, receiver, url)
        .await
        .unwrap());
    server.await.unwrap();
    assert_eq!(std::fs::read(dir.join("artifact")).unwrap(), b"abcdef");
    std::fs::remove_dir_all(&manager.root).unwrap();
}
#[tokio::test]
async fn wrong_range_keeps_partial_and_checksum_failure_never_installs() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"abc");
    let (url, task) = server("HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/6\r\nConnection: close\r\n\r\ndef", "range: bytes=3-").await;
    let (_sender, receiver) = watch::channel(false);
    assert_eq!(
        transfer::download(manager.clone(), &model, receiver, url).await,
        Err(ModelFailure::SizeMismatch)
    );
    task.await.unwrap();
    assert_eq!(std::fs::read(dir.join("artifact.part")).unwrap(), b"abc");
    let (url, task) = server(
        "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdex",
        "range: bytes=3-",
    )
    .await;
    let (_sender, receiver) = watch::channel(false);
    assert_eq!(
        transfer::download(manager.clone(), &model, receiver, url).await,
        Err(ModelFailure::ChecksumMismatch)
    );
    task.await.unwrap();
    assert!(!dir.join("artifact").exists());
    assert!(!dir.join("artifact.part").exists());
    std::fs::remove_dir_all(&manager.root).unwrap();
}
#[tokio::test]
async fn complete_partial_resumes_offline_and_corrupt_installed_file_is_not_ready() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"abcdef");
    let (_sender, receiver) = watch::channel(false);
    assert!(
        transfer::download(manager.clone(), &model, receiver, "not-used".into())
            .await
            .unwrap()
    );
    std::fs::write(dir.join("artifact"), b"abcdex").unwrap();
    let fresh = Arc::new(ModelManager::with_catalog(
        manager.root.clone(),
        vec![model],
    ));
    let view = fresh.list().await.unwrap().remove(0);
    assert_eq!(view.status, ModelStatus::Failed);
    assert_eq!(view.failure, Some(ModelFailure::ChecksumMismatch));
    assert!(fresh.remove("../settings.db").await.is_err());
    std::fs::remove_dir_all(&manager.root).unwrap();
}
#[tokio::test]
async fn cancellation_interrupts_a_stalled_request_and_preserves_partial() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"abc");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let (sender, receiver) = watch::channel(false);
    let task = tokio::spawn({
        let manager = manager.clone();
        async move { transfer::download(manager, &model, receiver, url).await }
    });
    let (_stream, _) = listener.accept().await.unwrap();
    sender.send(true).unwrap();
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    assert_eq!(std::fs::read(dir.join("artifact.part")).unwrap(), b"abc");
    assert!(!dir.join("artifact").exists());
    std::fs::remove_dir_all(&manager.root).unwrap();
}
#[tokio::test]
async fn a_second_download_and_removal_cannot_race_an_active_writer() {
    let (manager, _) = fixture();
    manager.initialize().await.unwrap();
    let (cancel, _receiver) = watch::channel(false);
    manager.inner.lock().unwrap().active = Some(Active {
        id: "qwen3-tts-06-0".into(),
        cancel,
    });
    assert_eq!(
        manager.start("qwen3-tts-06-0").await,
        Err(ModelFailure::Busy)
    );
    assert_eq!(
        manager.remove("qwen3-tts-06-0").await,
        Err(ModelFailure::Busy)
    );
}
#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_not_followed_or_removed() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"abc");
    std::fs::remove_file(dir.join("artifact.part")).unwrap();
    let protected = manager.root.join("settings.db");
    std::fs::write(&protected, b"keep").unwrap();
    std::os::unix::fs::symlink(&protected, dir.join("artifact.part")).unwrap();
    assert!(matches!(
        manager.list().await,
        Err(ModelFailure::UnsafePath)
    ));
    assert_eq!(std::fs::read(&protected).unwrap(), b"keep");
    std::fs::remove_dir_all(&manager.root).unwrap();
}

#[test]
fn catalog_is_pinned_and_contains_only_safe_model_files() {
    let models = catalog::catalog();
    assert!(models.iter().map(|m| u64::from(m.bytes)).sum::<u64>() < 3_000_000_000);
    for model in models {
        assert_eq!(model.revision.len(), 40);
        assert!(model.revision.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(model.sha256.len(), 64);
        assert!(model.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(model.bytes > 0 && !model.license.is_empty());
        assert!(model.experimental);
        assert!(std::path::Path::new(&model.filename)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))));
    }
}

#[tokio::test]
async fn truncated_responses_keep_only_received_bytes_for_later_resume() {
    let (manager, model) = fixture();
    let dir = partial(&manager, &model, b"");
    let (url, task) = server(
        "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc",
        "range: bytes=0-",
    )
    .await;
    let (_sender, receiver) = watch::channel(false);
    assert_eq!(
        transfer::download(manager.clone(), &model, receiver, url).await,
        Err(ModelFailure::Network)
    );
    task.await.unwrap();
    assert!(!dir.join("artifact").exists());
    let bytes = std::fs::read(dir.join("artifact.part")).unwrap();
    assert!(b"abc".starts_with(&bytes));
    let restarted = Arc::new(ModelManager::with_catalog(
        manager.root.clone(),
        vec![model],
    ));
    let view = restarted.list().await.unwrap().remove(0);
    assert_eq!(view.status, ModelStatus::Paused);
    assert_eq!(view.downloaded_bytes as usize, bytes.len());
    std::fs::remove_dir_all(&manager.root).unwrap();
}

#[tokio::test]
async fn safetensors_use_their_own_verified_cache_artifact_and_resume_path() {
    let (old, mut model) = fixture();
    model.filename = "model.safetensors".into();
    let manager = Arc::new(ModelManager::with_catalog(
        old.root.clone(),
        vec![model.clone()],
    ));
    let dir = manager.root.join(model.directory());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(model.partial_name()), b"abc").unwrap();
    assert_eq!(manager.list().await.unwrap()[0].status, ModelStatus::Paused);
    let (url, server) = server("HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef", "range: bytes=3-").await;
    let (_sender, receiver) = watch::channel(false);
    transfer::download(manager.clone(), &model, receiver, url)
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(std::fs::read(dir.join("artifact")).unwrap(), b"abcdef");
    let fresh = Arc::new(ModelManager::with_catalog(
        manager.root.clone(),
        vec![model.clone()],
    ));
    assert_eq!(
        fresh.list().await.unwrap()[0].status,
        ModelStatus::Downloaded
    );
    fresh.remove(&model.id).await.unwrap();
    assert!(!dir.join(model.artifact_name()).exists());
    std::fs::remove_dir_all(&manager.root).unwrap();
}

#[tokio::test]
async fn presence_check_does_not_bypass_checksum_verification() {
    let (manager, spec) = fixture();
    assert!(!manager.artifact_present(&spec.id));
    let directory = manager.root.join(spec.directory());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(spec.artifact_name()), b"xxxxxx").unwrap();
    assert!(manager.artifact_present(&spec.id));
    assert!(manager.downloaded_artifact(&spec.id).is_none());
    let views = manager.list().await.unwrap();
    assert_eq!(views[0].status, ModelStatus::Failed);
    assert!(!manager.artifact_present(&spec.id));
    assert!(manager.downloaded_artifact(&spec.id).is_none());
    std::fs::remove_dir_all(&manager.root).unwrap();
}
