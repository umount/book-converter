use super::*;
use crate::app::{
    contracts::{EntitySelection, ProjectKind},
    requests::{LanguagePair, ProjectChoices},
};
struct Fixture {
    root: PathBuf,
    manager: ProjectManager,
    project: ProjectId,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("audio-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.txt");
        std::fs::write(
            &source,
            "Глава 1. Начало\n\nПервый абзац.\n\nГлава 2. Продолжение\n\nВторой абзац.",
        )
        .unwrap();
        let manager = ProjectManager::new(root.join("app"));
        let preview = manager.inspect_source(ProjectKind::Book, &source).unwrap();
        let project = manager
            .create(
                &preview.import_id.0,
                &ProjectChoices {
                    name: "Test".into(),
                    languages: LanguagePair {
                        source: Some("ru".into()),
                        target: "en".into(),
                    },
                    processing_profile_id: None,
                },
            )
            .unwrap()
            .id;
        Self {
            root,
            manager,
            project,
        }
    }
    fn args(&self) -> AudioStartArgs {
        AudioStartArgs {
            project_id: self.project.clone(),
            selection: EntitySelection::All,
            text: AudioText::Original,
            voice: "Ryan".into(),
            device: AudioDevice::Cpu,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn snapshots_originals_in_order_and_never_substitutes_missing_translation() {
    let f = Fixture::new();
    let lease = f.manager.lease(&f.project).unwrap();
    let mut args = f.args();
    let input = text::snapshot(&lease, &args).unwrap();
    assert_eq!(input.language, "Russian");
    let spoken = input
        .chapters
        .iter()
        .flat_map(|c| c.chunks.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(spoken.find("Первый абзац").unwrap() < spoken.find("Второй абзац").unwrap());
    args.text = AudioText::Translation;
    assert_eq!(
        text::snapshot(&lease, &args).err().unwrap().params["reason"],
        "audioIncomplete"
    );
    args.voice = "invalid".into();
    assert!(text::snapshot(&lease, &args).is_err());
}
#[test]
fn recovery_and_export_keep_existing_destination_and_project_safe() {
    let f = Fixture::new();
    let service = Narration::new(f.root.join("audio"));
    let id = uuid::Uuid::new_v4().to_string();
    let dir = service.project_dir(&f.project).unwrap().join(&id);
    std::fs::create_dir(&dir).unwrap();
    let mut input = text::snapshot(&f.manager.lease(&f.project).unwrap(), &f.args()).unwrap();
    input.chapters.truncate(1);
    let mut view = AudioJobView {
        id: id.clone(),
        project_id: f.project.clone(),
        state: AudioState::Running,
        voice: "Ryan".into(),
        device: AudioDevice::Cpu,
        text: AudioText::Original,
        language: "Russian".into(),
        completed_chunks: 1,
        total_chunks: 1,
        completed_chapters: 1,
        total_chapters: 1,
        current_chapter: "Test".into(),
        error: None,
        created_at: "1".into(),
    };
    write_json(&dir.join("status.json"), &view).unwrap();
    write_json(&dir.join("input.json"), &input).unwrap();
    assert_eq!(
        service.list(&f.project).unwrap()[0].state,
        AudioState::Interrupted
    );
    view.state = AudioState::Succeeded;
    write_json(&dir.join("status.json"), &view).unwrap();
    std::fs::create_dir(dir.join("audio")).unwrap();
    std::fs::write(dir.join("audio/00001.mp3"), b"test-mp3").unwrap();
    use sha2::{Digest, Sha256};
    write_json(
        &dir.join("audio/00001.json"),
        &serde_json::json!({"sha256": format!("{:x}", Sha256::digest(b"test-mp3"))}),
    )
    .unwrap();
    let args = AudioExportArgs {
        project_id: f.project.clone(),
        job_id: id.clone(),
        destination: f.root.to_string_lossy().into_owned(),
    };
    let output = PathBuf::from(service.export(&f.manager, &args).unwrap());
    assert_eq!(
        std::fs::read(output.join("00001.mp3")).unwrap(),
        b"test-mp3"
    );
    assert!(output.join("book.m3u8").is_file());
    assert!(service.export(&f.manager, &args).is_err());
    std::fs::remove_dir_all(&output).unwrap();
    std::fs::write(dir.join("audio/00001.mp3"), b"corrupt").unwrap();
    assert!(service.export(&f.manager, &args).is_err());
    assert!(!output.exists());
    assert!(service
        .export(
            &f.manager,
            &AudioExportArgs {
                destination: dir.to_string_lossy().into_owned(),
                ..args
            }
        )
        .is_err());
    assert!(service
        .directory(&AudioJobArgs {
            project_id: f.project.clone(),
            job_id: "../outside".into()
        })
        .is_err());
    service.remove_project(&f.project).unwrap();
    assert!(!dir.exists());
    assert!(f.manager.lease(&f.project).is_ok());
}
#[test]
fn one_runtime_reservation_covers_all_projects() {
    let f = Fixture::new();
    let service = Narration::new(f.root.join("audio"));
    let cancel = service.reserve(&f.project, "first").unwrap();
    assert!(service.reserve(&ProjectId::new(), "second").is_err());
    service
        .active
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .cancel
        .send(true)
        .unwrap();
    assert!(*cancel.borrow());
}

#[test]
fn runtime_availability_does_not_bypass_integrity_at_job_admission() {
    let f = Fixture::new();
    let root = f.root.join("tts-runtime");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join(if cfg!(windows) {
            "book-tts.exe"
        } else {
            "book-tts"
        }),
        b"corrupt runtime",
    )
    .unwrap();
    write_json(
        &root.join("manifest.json"),
        &serde_json::json!({
            "version": 1, "platform": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "sha256": "0".repeat(64),
        }),
    )
    .unwrap();
    assert!(Runtime::locate_pack(&f.root, false).is_ok());
    assert!(Runtime::locate_pack(&f.root, true).is_err());
}

#[cfg(unix)]
fn fake_runtime(f: &Fixture, body: &str) -> Runtime {
    use std::os::unix::fs::PermissionsExt;
    let executable = f.root.join("test-worker");
    std::fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    Runtime::for_test(executable)
}

#[cfg(unix)]
async fn wait_until(check: impl Fn() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !check() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("worker did not settle");
}

#[cfg(unix)]
#[tokio::test]
async fn pause_resume_and_project_deletion_release_the_worker_lease() {
    let f = Fixture::new();
    let manager = Arc::new(ProjectManager::new(f.root.join("app")));
    let models = Arc::new(ModelManager::with_catalog(f.root.join("models"), vec![]));
    let service = Arc::new(Narration::new(f.root.join("audio")));
    let runtime = fake_runtime(&f, &format!(
        "echo $$ > '{}/started'\necho '{{\"event\":\"loaded\",\"device\":\"cpu\"}}'\nwhile IFS= read -r line; do\ncase \"$line\" in\n*'\"command\":\"run\"'*) echo run >> '{}/runs';;\n*'\"command\":\"pause\"'*) echo '{{\"event\":\"paused\"}}';;\nesac\ndone",
        f.root.display(), f.root.display()
    ));
    let job = service
        .start(manager.clone(), models.clone(), runtime, f.args())
        .await
        .unwrap();
    let args = AudioJobArgs {
        project_id: f.project.clone(),
        job_id: job.id.clone(),
    };
    let dir = service.directory(&args).unwrap();
    wait_until(|| f.root.join("runs").is_file()).await;
    let pid = std::fs::read_to_string(f.root.join("started")).unwrap();
    assert!(service.unload_engine().await.is_err());
    service.cancel(&args).unwrap();
    wait_until(|| service.list(&f.project).unwrap()[0].state == AudioState::Paused).await;
    assert_eq!(service.engine_view().state, AudioEngineState::Ready);
    let snapshot = std::fs::read(dir.join("input.json")).unwrap();
    let resumed = service
        .resume(
            manager.clone(),
            models,
            Runtime::for_test(f.root.join("test-worker")),
            args,
        )
        .await
        .unwrap();
    assert_eq!(resumed.id, job.id);
    wait_until(|| {
        std::fs::read_to_string(f.root.join("runs"))
            .unwrap()
            .lines()
            .count()
            == 2
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(f.root.join("started")).unwrap(),
        pid
    );
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), snapshot);
    let project = f.project.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || manager.delete(&project)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        service.list(&f.project).unwrap()[0].state,
        AudioState::Paused
    );
    assert!(service.active.lock().unwrap().is_none());
    assert_eq!(service.engine_view().state, AudioEngineState::Unloaded);
    #[cfg(target_os = "linux")]
    assert!(!Path::new("/proc").join(pid.trim()).exists());
    service.remove_project(&f.project).unwrap();
    assert!(!dir.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn invalid_worker_progress_fails_the_job_and_releases_admission() {
    let f = Fixture::new();
    let manager = Arc::new(ProjectManager::new(f.root.join("app")));
    let models = Arc::new(ModelManager::with_catalog(f.root.join("models"), vec![]));
    let service = Arc::new(Narration::new(f.root.join("audio")));
    let runtime = fake_runtime(
        &f,
        "echo '{\"event\":\"loaded\",\"device\":\"cpu\"}'\nread -r line\necho '{\"event\":\"chunk\",\"completed\":999999,\"chapter\":\"invalid\"}'",
    );
    service
        .start(manager, models, runtime, f.args())
        .await
        .unwrap();
    wait_until(|| service.list(&f.project).unwrap()[0].state == AudioState::Failed).await;
    assert_eq!(
        service.list(&f.project).unwrap()[0]
            .error
            .as_ref()
            .unwrap()
            .params["reason"],
        "audioWorker"
    );
    assert!(service.active.lock().unwrap().is_none());
    assert_eq!(service.engine_view().state, AudioEngineState::Failed);
}

#[cfg(unix)]
#[tokio::test]
async fn preloaded_model_serves_multiple_jobs_until_explicit_unload() {
    let f = Fixture::new();
    let manager = Arc::new(ProjectManager::new(f.root.join("app")));
    let models = Arc::new(ModelManager::with_catalog(f.root.join("models"), vec![]));
    let service = Arc::new(Narration::new(f.root.join("audio")));
    let input = text::snapshot(&manager.lease(&f.project).unwrap(), &f.args()).unwrap();
    let chunks: usize = input
        .chapters
        .iter()
        .map(|chapter| chapter.chunks.len())
        .sum();
    let runtime = fake_runtime(&f, &format!(
        "echo $$ >> '{}/loads'\necho '{{\"event\":\"loaded\",\"device\":\"cpu\"}}'\nwhile IFS= read -r line; do\necho '{{\"event\":\"chunk\",\"completed\":{},\"chapter\":\"Test\"}}'\necho '{{\"event\":\"chapter\",\"completed\":{}}}'\necho '{{\"event\":\"done\"}}'\ndone",
        f.root.display(), chunks, input.chapters.len()
    ));
    service
        .load_engine(models.clone(), runtime, AudioDevice::Cpu)
        .unwrap();
    wait_until(|| service.engine_view().state == AudioEngineState::Ready).await;
    for _ in 0..2 {
        let job = service
            .start(
                manager.clone(),
                models.clone(),
                Runtime::for_test(f.root.join("test-worker")),
                f.args(),
            )
            .await
            .unwrap();
        wait_until(|| {
            service
                .list(&f.project)
                .unwrap()
                .iter()
                .any(|view| view.id == job.id && view.state == AudioState::Succeeded)
        })
        .await;
        assert_eq!(service.engine_view().state, AudioEngineState::Ready);
    }
    assert_eq!(
        std::fs::read_to_string(f.root.join("loads"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    service.unload_engine().await.unwrap();
    assert_eq!(service.engine_view().state, AudioEngineState::Unloaded);
    assert_eq!(
        std::fs::read_dir(f.root.join("audio/.engine"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[tokio::test]
async fn model_loading_failure_is_visible_and_can_be_retried() {
    let f = Fixture::new();
    let models = Arc::new(ModelManager::with_catalog(f.root.join("models"), vec![]));
    let service = Arc::new(Narration::new(f.root.join("audio")));
    let runtime = fake_runtime(
        &f,
        "echo '{\"event\":\"error\",\"reason\":\"audioMemory\"}'",
    );
    service
        .load_engine(models.clone(), runtime, AudioDevice::Cpu)
        .unwrap();
    wait_until(|| service.engine_view().state == AudioEngineState::Failed).await;
    assert_eq!(
        service.engine_view().error.unwrap().params["reason"],
        "audioMemory"
    );
    let runtime = fake_runtime(
        &f,
        "echo '{\"event\":\"loaded\",\"device\":\"cpu\"}'\nread -r line",
    );
    service
        .load_engine(models, runtime, AudioDevice::Auto)
        .unwrap();
    wait_until(|| service.engine_view().state == AudioEngineState::Ready).await;
    assert_eq!(service.engine_view().device, Some(AudioDevice::Cpu));
    service.unload_engine().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn preview_during_narration_exports_the_heard_sample_without_loading_model() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use sha2::{Digest, Sha256};
    let f = Fixture::new();
    let service = Narration::new(f.root.join("audio"));
    let id = uuid::Uuid::new_v4().to_string();
    let directory = service.project_dir(&f.project).unwrap().join(&id);
    std::fs::create_dir(&directory).unwrap();
    let mut view = AudioJobView {
        id: id.clone(),
        project_id: f.project.clone(),
        state: AudioState::Running,
        voice: "Ryan".into(),
        device: AudioDevice::Cpu,
        text: AudioText::Original,
        language: "Russian".into(),
        completed_chunks: 0,
        total_chunks: 2,
        completed_chapters: 0,
        total_chapters: 1,
        current_chapter: "Chapter".into(),
        error: None,
        created_at: "1".into(),
    };
    let _reservation = service.reserve(&f.project, &id).unwrap();
    let args = AudioJobArgs {
        project_id: f.project.clone(),
        job_id: id.clone(),
    };
    write_json(&directory.join("status.json"), &view).unwrap();
    let runtime = fake_runtime(&f, "exit 1");
    assert_eq!(
        service
            .preview(&f.manager, runtime, &args)
            .await
            .unwrap_err()
            .params["reason"],
        "audioPreviewUnavailable"
    );
    view.completed_chunks = 1;
    write_json(&directory.join("status.json"), &view).unwrap();
    // Python tests cover MP3 decoding; this exercises IPC, artifact validation and export.
    let bytes = b"encoded-sample";
    std::fs::write(f.root.join("sample.mp3"), bytes).unwrap();
    write_json(&f.root.join("receipt.json"), &serde_json::json!({
        "title": "Chapter", "durationSeconds": 12.0, "sha256": format!("{:x}", Sha256::digest(bytes)),
    })).unwrap();
    let runtime = fake_runtime(&f, &format!(
        "cp '{}/sample.mp3' \"$5/audio.mp3\"\ncp '{}/receipt.json' \"$5/preview.json\"\necho '{{\"event\":\"preview\"}}'",
        f.root.display(), f.root.display()
    ));
    let preview = service.preview(&f.manager, runtime, &args).await.unwrap();
    assert_eq!(
        preview.audio_url,
        format!("data:audio/mpeg;base64,{}", STANDARD.encode(bytes))
    );
    assert_eq!(service.engine_view().state, AudioEngineState::Unloaded);
    assert_eq!(service.read_view(&args).unwrap().state, AudioState::Running);
    let export = AudioPreviewExportArgs {
        project_id: f.project.clone(),
        job_id: id,
        preview_id: preview.preview_id.clone(),
        destination: f.root.to_string_lossy().into_owned(),
    };
    let output = PathBuf::from(service.export_preview(&f.manager, &export).unwrap());
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    assert!(service.export_preview(&f.manager, &export).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    std::fs::remove_file(&output).unwrap();
    assert!(service
        .export_preview(
            &f.manager,
            &AudioPreviewExportArgs {
                destination: directory.to_string_lossy().into_owned(),
                ..export.clone()
            }
        )
        .is_err());
    assert!(service
        .export_preview(
            &f.manager,
            &AudioPreviewExportArgs {
                preview_id: "../outside".into(),
                ..export.clone()
            }
        )
        .is_err());
    std::fs::write(
        directory
            .join("previews")
            .join(preview.preview_id)
            .join("audio.mp3"),
        b"corrupt",
    )
    .unwrap();
    assert!(service.export_preview(&f.manager, &export).is_err());
    assert!(!output.exists());
}
