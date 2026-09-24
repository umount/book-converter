use super::*;
use reqwest::{header, StatusCode};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};
use tokio::io::AsyncWriteExt;

pub(super) fn verify(
    path: &Path,
    spec: &ModelSpec,
    cancel: Option<&watch::Receiver<bool>>,
) -> Result<(), ModelFailure> {
    if regular_size(path)? != Some(u64::from(spec.bytes)) {
        return Err(ModelFailure::SizeMismatch);
    }
    let mut file = std::fs::File::open(path).map_err(|_| ModelFailure::Storage)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancel.is_some_and(|v| *v.borrow()) {
            return Err(ModelFailure::Busy);
        }
        let count = file.read(&mut buffer).map_err(|_| ModelFailure::Storage)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if format!("{:x}", hash.finalize()) != spec.sha256 {
        return Err(ModelFailure::ChecksumMismatch);
    }
    Ok(())
}
fn trusted_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| {
            host == "huggingface.co"
                || host.ends_with(".huggingface.co")
                || host == "hf.co"
                || host.ends_with(".hf.co")
        })
}
fn response_offset(
    status: StatusCode,
    range: Option<&str>,
    offset: u64,
    total: u64,
) -> Result<u64, ModelFailure> {
    if status == StatusCode::OK {
        return Ok(0);
    }
    if status != StatusCode::PARTIAL_CONTENT {
        return Err(ModelFailure::Network);
    }
    let expected = format!("bytes {offset}-{}/{total}", total - 1);
    if range != Some(expected.as_str()) {
        return Err(ModelFailure::SizeMismatch);
    }
    Ok(offset)
}
pub(super) async fn download(
    manager: Arc<ModelManager>,
    spec: &ModelSpec,
    mut cancel: watch::Receiver<bool>,
    url: String,
) -> Result<bool, ModelFailure> {
    check_directory(&manager.root)?;
    std::fs::create_dir_all(&manager.root).map_err(|_| ModelFailure::Storage)?;
    let directory = manager.root.join(spec.directory());
    check_directory(&directory)?;
    std::fs::create_dir_all(&directory).map_err(|_| ModelFailure::Storage)?;
    let partial = directory.join(spec.partial_name());
    let final_path = directory.join(spec.artifact_name());
    let mut offset = regular_size(&partial)?.unwrap_or(0);
    regular_size(&final_path)?;
    if offset > u64::from(spec.bytes) {
        std::fs::remove_file(&partial).map_err(|_| ModelFailure::Storage)?;
        offset = 0;
    }
    if *cancel.borrow() {
        return Ok(false);
    }
    if offset < u64::from(spec.bytes) {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 || !trusted_url(attempt.url()) {
                    attempt.error("Untrusted model redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| ModelFailure::Network)?;
        let request = client
            .get(url)
            .header(header::ACCEPT_ENCODING, "identity")
            .header(header::RANGE, format!("bytes={offset}-"));
        let mut response = tokio::select! { biased; _ = cancel.changed() => return Ok(false), response = request.send() => response.map_err(|_| ModelFailure::Network)? };
        offset = response_offset(
            response.status(),
            response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            offset,
            u64::from(spec.bytes),
        )?;
        if response
            .content_length()
            .is_some_and(|len| len != u64::from(spec.bytes) - offset)
        {
            return Err(ModelFailure::SizeMismatch);
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(&partial)
            .await
            .map_err(|_| ModelFailure::Storage)?;
        manager.progress(&spec.id, ModelStatus::Downloading, offset as u32);
        loop {
            let chunk = tokio::select! { biased; _ = cancel.changed() => { file.flush().await.map_err(|_| ModelFailure::Storage)?; return Ok(false); }, chunk = response.chunk() => chunk };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    // Tokio file writes may still be queued after write_all returns.
                    // Drain them before releasing the writer or reporting resumable size.
                    file.flush().await.map_err(|_| ModelFailure::Storage)?;
                    return Err(ModelFailure::Network);
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            if offset + chunk.len() as u64 > u64::from(spec.bytes) {
                file.flush().await.map_err(|_| ModelFailure::Storage)?;
                return Err(ModelFailure::SizeMismatch);
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| ModelFailure::Storage)?;
            offset += chunk.len() as u64;
            manager.progress(&spec.id, ModelStatus::Downloading, offset as u32);
        }
        file.sync_all().await.map_err(|_| ModelFailure::Storage)?;
    }
    if *cancel.borrow() {
        return Ok(false);
    }
    manager.progress(&spec.id, ModelStatus::Verifying, offset as u32);
    let (path, expected, cancelled) = (partial.clone(), spec.clone(), cancel.clone());
    let verified = tokio::task::spawn_blocking(move || verify(&path, &expected, Some(&cancelled)))
        .await
        .map_err(|_| ModelFailure::Storage)?;
    if *cancel.borrow() {
        return Ok(false);
    }
    if let Err(error) = verified {
        if error == ModelFailure::ChecksumMismatch {
            std::fs::remove_file(&partial).map_err(|_| ModelFailure::Storage)?;
            manager.progress(&spec.id, ModelStatus::Failed, 0);
        }
        return Err(error);
    }
    // An existing corrupt final file is never treated as installed. Remove it only
    // after the replacement has been completely downloaded and verified.
    if regular_size(&final_path)?.is_some() {
        std::fs::remove_file(&final_path).map_err(|_| ModelFailure::Storage)?;
    }
    std::fs::rename(&partial, &final_path).map_err(|_| ModelFailure::Storage)?;
    Ok(true)
}
