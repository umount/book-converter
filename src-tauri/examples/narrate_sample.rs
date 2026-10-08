//! Explicit end-to-end smoke test in an isolated app directory, using real weights.
use book_converter_lib::{
    app::{
        contracts::{EntitySelection, ProjectKind},
        requests::{LanguagePair, ProjectChoices},
    },
    models::ModelManager,
    narration::{
        contracts::{AudioDevice, AudioExportArgs, AudioStartArgs, AudioState, AudioText},
        Narration, Runtime,
    },
    project::lifecycle::ProjectManager,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "usage: narrate_sample APP_DATA MODEL_CACHE RESOURCE_DIR SOURCE_TXT EXPORT_DIR".into(),
        );
    }
    let manager = Arc::new(ProjectManager::new(args[0].clone()));
    let preview = manager
        .inspect_source(ProjectKind::Book, &args[3])
        .map_err(|e| format!("{e:?}"))?;
    let project = manager
        .create(
            &preview.import_id.0,
            &ProjectChoices {
                name: "Narration smoke test".into(),
                languages: LanguagePair {
                    source: Some("ru".into()),
                    target: "en".into(),
                },
                processing_profile_id: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let models = Arc::new(ModelManager::new(args[1].clone()));
    let service = Arc::new(Narration::new(args[0].join("audiobooks")));
    let runtime = Runtime::discover(&args[2]).map_err(|e| format!("{e:?}"))?;
    let job = service
        .start(
            manager.clone(),
            models,
            runtime,
            AudioStartArgs {
                project_id: project.id.clone(),
                selection: EntitySelection::All,
                text: AudioText::Original,
                voice: "Ryan".into(),
                device: AudioDevice::Cpu,
            },
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    let mut previous = None;
    loop {
        let current = service
            .list(&project.id)
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .find(|v| v.id == job.id)
            .ok_or("job missing")?;
        if previous.as_ref() != Some(&(current.state.clone(), current.completed_chunks)) {
            println!(
                "{:?}: {}/{} fragments, {}/{} chapters",
                current.state,
                current.completed_chunks,
                current.total_chunks,
                current.completed_chapters,
                current.total_chapters
            );
            previous = Some((current.state.clone(), current.completed_chunks));
        }
        match current.state {
            AudioState::Succeeded => break,
            AudioState::Running => tokio::time::sleep(Duration::from_secs(1)).await,
            _ => return Err(format!("Narration stopped: {:?}", current.error).into()),
        }
    }
    let destination = service
        .export(
            &manager,
            &AudioExportArgs {
                project_id: project.id,
                job_id: job.id,
                destination: args[4].to_string_lossy().into_owned(),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    println!("Exported: {destination}");
    Ok(())
}
