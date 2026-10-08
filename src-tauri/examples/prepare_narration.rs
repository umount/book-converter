//! Explicit developer-only model download. Uses the same pinned, resumable installer as the UI.
use book_converter_lib::models::{ModelManager, ModelStatus};
use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("model cache directory"));
    let manager = Arc::new(ModelManager::new(root));
    manager.start_bundle().await.expect("start download");
    while manager.busy() {
        let views = manager.list().await.expect("list files");
        let bytes: u64 = views.iter().map(|v| u64::from(v.downloaded_bytes)).sum();
        eprintln!("Downloaded {} MB", bytes / 1_000_000);
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }
    let views = manager.list().await.unwrap();
    for view in &views {
        assert_eq!(
            view.status,
            ModelStatus::Downloaded,
            "{}: {:?}",
            view.model.name,
            view.failure
        );
    }
    if let Some(destination) = std::env::args().nth(2) {
        manager
            .materialize(&PathBuf::from(destination))
            .await
            .expect("prepare model");
    }
    println!("Verified {} files", views.len());
}
