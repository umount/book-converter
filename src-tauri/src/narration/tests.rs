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
