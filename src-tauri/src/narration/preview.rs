use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    title: String,
    duration_seconds: f64,
    sha256: String,
}

fn read_sample(directory: &Path) -> Result<(Receipt, Vec<u8>), AppError> {
    real_dir(directory)?;
    let receipt: Receipt = read_json(&directory.join("preview.json"))?;
    let path = directory.join("audio.mp3");
    let meta = std::fs::symlink_metadata(&path).map_err(io_error)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() == 0
        || meta.len() > 1_000_000
        || !receipt.duration_seconds.is_finite()
        || receipt.duration_seconds <= 0.0
        || receipt.duration_seconds > 30.0
    {
        return Err(failure("audioStorage"));
    }
    let bytes = std::fs::read(path).map_err(io_error)?;
    if format!("{:x}", Sha256::digest(&bytes)) != receipt.sha256 {
        return Err(failure("audioStorage"));
    }
    Ok((receipt, bytes))
}

impl Narration {
    pub async fn preview(
        &self,
        manager: &ProjectManager,
        runtime: Runtime,
        args: &AudioJobArgs,
    ) -> Result<AudioPreviewView, AppError> {
        let lease = manager.lease(&args.project_id)?;
        if self.read_view(args)?.completed_chunks == 0 {
            return Err(failure("audioPreviewUnavailable"));
        }
        let directory = self.directory(args)?;
        let previews = directory.join("previews");
        std::fs::create_dir_all(&previews).map_err(io_error)?;
        real_dir(&previews)?;
        let preview_id = uuid::Uuid::new_v4().to_string();
        let output = previews.join(&preview_id);
        std::fs::create_dir(&output).map_err(io_error)?;
        let result = async {
            runtime.preview(&lease, &directory, &output).await?;
            let (receipt, bytes) = read_sample(&output)?;
            Ok(AudioPreviewView {
                preview_id,
                title: receipt.title,
                duration_seconds: receipt.duration_seconds,
                audio_url: format!("data:audio/mpeg;base64,{}", STANDARD.encode(bytes)),
            })
        }
        .await;
        if result.is_err() {
            let _ = std::fs::remove_dir_all(output);
        }
        result
    }

    pub fn export_preview(
        &self,
        manager: &ProjectManager,
        args: &AudioPreviewExportArgs,
    ) -> Result<String, AppError> {
        let _lease = manager.lease(&args.project_id)?;
        valid_id(&args.preview_id)?;
        let directory = self
            .directory(&AudioJobArgs {
                project_id: args.project_id.clone(),
                job_id: args.job_id.clone(),
            })?
            .join("previews");
        real_dir(&directory)?;
        let (_, bytes) = read_sample(&directory.join(&args.preview_id))?;
        let parent = Path::new(&args.destination)
            .canonicalize()
            .map_err(io_error)?;
        manager.validate_export_directory(&parent)?;
        if parent.starts_with(self.root.canonicalize().map_err(io_error)?) {
            return Err(AppError::invalid("destination"));
        }
        let output = parent.join(format!(
            "fragment-{}-{}.mp3",
            &args.job_id[..8],
            &args.preview_id[..8]
        ));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    AppError::invalid("destinationExists")
                } else {
                    io_error(e)
                }
            })?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&output);
            return Err(io_error(error));
        }
        Ok(output.to_string_lossy().into_owned())
    }
}
