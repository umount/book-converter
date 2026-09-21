//! Portable `.bcproj` archive commands.

use crate::dto::{err, ImportedProject};
use crate::session::{project_dir, Manifest};

#[tauri::command]
pub async fn export_project(project_id: String, out_path: String) -> Result<(), String> {
    use std::io::Write as _;

    let directory = project_dir(&project_id).map_err(err)?;
    let manifest = std::fs::read(directory.join("project.json")).map_err(err)?;
    let database = std::fs::read(directory.join("progress.db")).map_err(err)?;

    let file = std::fs::File::create(&out_path).map_err(err)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [("project.json", &manifest), ("progress.db", &database)] {
        archive.start_file(name, options).map_err(err)?;
        archive.write_all(bytes).map_err(err)?;
    }
    archive.finish().map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn import_project(
    project_id: String,
    archive_path: String,
) -> Result<ImportedProject, String> {
    use std::io::Read as _;

    let directory = project_dir(&project_id).map_err(err)?;
    std::fs::create_dir_all(&directory).map_err(err)?;
    let file = std::fs::File::open(&archive_path).map_err(err)?;
    let mut archive = zip::ZipArchive::new(file).map_err(err)?;

    let mut name = "Imported project".to_string();
    let mut source_path = String::new();
    let mut has_database = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(err)?;
        let entry_name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(err)?;
        match entry_name.as_str() {
            "project.json" => {
                if let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) {
                    name = manifest.name;
                    source_path = manifest.source_path;
                }
                std::fs::write(directory.join("project.json"), bytes).map_err(err)?;
            }
            "progress.db" => {
                std::fs::write(directory.join("progress.db"), bytes).map_err(err)?;
                has_database = true;
            }
            _ => {}
        }
    }
    if !has_database {
        return Err("archive_no_book".into());
    }
    Ok(ImportedProject { name, source_path })
}
