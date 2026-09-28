//! Consistent structured book snapshots and atomic publication of exported files.
use crate::{
    app::{
        contracts::{AppError, ProjectKind},
        requests::{BookExportArgs, BookExportFormat, IncompletePolicy},
    },
    assets::store::AssetStore,
    export::{
        self, ChapterBody, ExportBlock, ExportImage, OutputFormat, OutputMeta, TranslatedChapter,
    },
    project::lifecycle::ProjectManager,
    storage::{repository::ProjectRepository, shared},
};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

fn storage_error(_: impl std::fmt::Display) -> AppError {
    AppError {
        code: crate::app::contracts::ErrorCode::Storage,
        message_key: "errors.export".into(),
        params: Default::default(),
        retryable: false,
    }
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn export_book(manager: &ProjectManager, args: &BookExportArgs) -> Result<(), AppError> {
    let descriptor = manager.open(&args.project_id)?;
    if descriptor.kind != ProjectKind::Book {
        return Err(AppError::invalid("projectKind"));
    }
    let destination_path = if args.format == BookExportFormat::Fb2 {
        let lower = args.destination.to_ascii_lowercase();
        if lower.ends_with(".zip") {
            PathBuf::from(&args.destination)
        } else if lower.ends_with(".fb2") {
            PathBuf::from(format!("{}.zip", args.destination))
        } else {
            PathBuf::from(format!("{}.fb2.zip", args.destination))
        }
    } else {
        PathBuf::from(&args.destination)
    };
    let destination = destination_path.as_path();
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize().map_err(storage_error)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or_else(|| AppError::invalid("destination"))?,
    );
    // Confirmation applies only to the path selected in the save dialog, not an
    // alternative filename produced by FB2 extension normalization.
    let overwrite = args.overwrite && destination_path == Path::new(&args.destination);
    if !overwrite && destination.try_exists().map_err(storage_error)? {
        return Err(AppError::invalid("destinationExists"));
    }
    manager.validate_export_directory(&parent)?;
    let lease = manager.lease(&args.project_id)?;
    let scratch = Scratch(parent.join(format!(".book-export-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&scratch.0).map_err(storage_error)?;
    let (chapters, mut meta) = lease.with_connection(|db, directory| {
        // Exports must not create files inside the project, including asset directories.
        if parent.starts_with(directory.canonicalize().map_err(storage_error)?) {
            return Err(AppError::invalid("destination"));
        }
        snapshot(db, directory, &scratch.0, args)
    })?;
    if meta.title.trim().is_empty() {
        meta.title = descriptor.name;
    }
    let format = match args.format {
        BookExportFormat::Txt => OutputFormat::Txt,
        BookExportFormat::Fb2 => OutputFormat::Fb2,
        BookExportFormat::Epub => OutputFormat::Epub,
        BookExportFormat::Pdf => OutputFormat::Pdf,
    };
    let temporary = scratch.0.join(format!("book.{}", format.ext()));
    export::export(&chapters, format, &meta, &temporary).map_err(storage_error)?;
    let temporary = if args.format == BookExportFormat::Fb2 {
        let archive = scratch.0.join("book.fb2.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).map_err(storage_error)?);
        let stem = destination
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("book");
        let entry = if stem.to_ascii_lowercase().ends_with(".fb2") {
            stem.to_string()
        } else {
            format!("{stem}.fb2")
        };
        zip.start_file(
            entry,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(storage_error)?;
        std::io::copy(
            &mut std::fs::File::open(&temporary).map_err(storage_error)?,
            &mut zip,
        )
        .map_err(storage_error)?;
        zip.finish().map_err(storage_error)?;
        archive
    } else {
        temporary
    };
    std::fs::File::open(&temporary)
        .and_then(|f| f.sync_all())
        .map_err(storage_error)?;
    if lease.cancelled() {
        return Err(AppError::invalid("projectClosing"));
    }
    if overwrite {
        // The old file remains intact until the completed export is ready to replace it.
        std::fs::rename(&temporary, &destination).map_err(storage_error)?;
    } else {
        std::fs::hard_link(&temporary, &destination).map_err(storage_error)?;
    }
    Ok(())
}

fn snapshot(
    db: &mut Connection,
    directory: &Path,
    scratch: &Path,
    args: &BookExportArgs,
) -> Result<(Vec<TranslatedChapter>, OutputMeta), AppError> {
    ProjectRepository::new(db, ProjectKind::Book)?;
    let tx = db.transaction().map_err(storage_error)?;
    let settings = shared::settings(&tx)?;
    let glossary = shared::glossary_revision(&tx)?;
    let ids = {
        let mut q = tx
            .prepare("SELECT id FROM book_chapters ORDER BY position")
            .map_err(storage_error)?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
    };
    // Preserve illustrations within the translated portion, not standalone image
    // chapters from much later in the book.
    let last_translated_position: Option<usize> = tx.query_row(
        "SELECT MAX(c.position) FROM book_chapters c WHERE EXISTS (SELECT 1 FROM book_translations t WHERE t.chapter_id=c.id AND t.target_language=?1)",
        [&settings.choices.target_language],
        |row| row.get(0),
    ).map_err(storage_error)?;
    let mut chapters = Vec::new();
    let mut images = HashMap::new();
    for id in args.selection.resolve(&ids)? {
        let (position, source_title, source_revision): (usize, String, i64) = tx
            .query_row(
                "SELECT position,source_title,revision FROM book_chapters WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(storage_error)?;
        // Export the saved translation shown in the editor. Revision freshness is a
        // processing concern: glossary changes must not hide already translated chapters.
        let translation: Option<(String, String)> = if args.incomplete_policy == IncompletePolicy::Reject {
            tx.query_row("SELECT id,translated_title FROM book_translations WHERE chapter_id=?1 AND target_language=?2 AND status='ready' AND (provenance='reference' OR (source_revision=?3 AND settings_revision=?4 AND glossary_revision=?5)) ORDER BY revision DESC LIMIT 1", rusqlite::params![id, settings.choices.target_language, source_revision, settings.revision.value()?, glossary.value()?], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?
        } else {
            tx.query_row("SELECT id,translated_title FROM book_translations WHERE chapter_id=?1 AND target_language=?2 ORDER BY revision DESC LIMIT 1", rusqlite::params![id, settings.choices.target_language], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?
        };
        let mut translated = HashMap::<String, String>::new();
        if let Some((translation_id, _)) = &translation {
            let mut q = tx.prepare("SELECT source_block_id,translated_text FROM book_translation_blocks WHERE translation_id=?1").map_err(storage_error)?;
            translated = q
                .query_map([translation_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(storage_error)?
                .collect::<Result<_, _>>()
                .map_err(storage_error)?;
        }
        let mut q = tx.prepare("SELECT id,kind,text,asset_id FROM book_source_blocks WHERE chapter_id=?1 ORDER BY position").map_err(storage_error)?;
        let rows = q
            .query_map([&id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        if args.incomplete_policy == IncompletePolicy::TranslatedOnly {
            let has_text = rows.iter().any(|(_, kind, text, _)| {
                kind != "image" && text.as_ref().is_some_and(|text| !text.trim().is_empty())
            });
            let has_images = rows.iter().any(|(_, kind, _, _)| kind == "image");
            let complete = translation.is_some()
                && rows.iter().all(|(block, kind, text, _)| {
                    kind == "image"
                        || text.as_ref().is_some_and(|text| text.trim().is_empty())
                        || translated.contains_key(block)
                });
            let beyond_translated_part = !has_text && translation.is_none()
                && last_translated_position.is_some_and(|last| position > last);
            if (has_text && !complete)
                || (!has_text && !has_images && translation.is_none())
                || beyond_translated_part
            {
                continue;
            }
        }
        let mut blocks = Vec::new();
        for (block, kind, text, asset) in rows {
            if kind == "image" {
                let asset = asset.ok_or_else(|| AppError::invalid("asset"))?;
                if !images.contains_key(&asset) {
                    let (relative, mime, size): (String, String, u64) = tx
                        .query_row(
                            "SELECT relative_path,mime,byte_length FROM assets WHERE id=?1",
                            [&asset],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                        )
                        .map_err(storage_error)?;
                    let extension = match mime.as_str() {
                        "image/png" => "png",
                        "image/jpeg" => "jpg",
                        "image/gif" => "gif",
                        "image/webp" => "webp",
                        _ => return Err(AppError::invalid("assetMime")),
                    };
                    if relative != format!("assets/{asset}.{extension}") {
                        return Err(AppError::invalid("assetPath"));
                    }
                    let bytes =
                        AssetStore::read_path(&directory.join(relative)).map_err(storage_error)?;
                    if bytes.len() as u64 != size
                        || format!("{:x}", Sha256::digest(&bytes)) != asset
                    {
                        return Err(AppError::invalid("assetHash"));
                    }
                    let path = scratch.join(format!("{asset}.{extension}"));
                    std::fs::write(&path, bytes).map_err(storage_error)?;
                    images.insert(
                        asset.clone(),
                        ExportImage {
                            path,
                            content_type: mime,
                        },
                    );
                }
                blocks.push(ExportBlock::Image(asset));
            } else {
                let source = text.ok_or_else(|| AppError::invalid("sourceText"))?;
                let text = match translated.remove(&block) {
                    Some(value) => value,
                    None if source.trim().is_empty()
                        || args.incomplete_policy == IncompletePolicy::Originals =>
                    {
                        source
                    }
                    None => return Err(AppError::invalid("incompleteTranslation")),
                };
                blocks.push(ExportBlock::Text(text));
            }
        }
        chapters.push(TranslatedChapter {
            index: position,
            number: None,
            title: translation.map(|t| t.1).unwrap_or(source_title),
            body: ChapterBody::Blocks(blocks),
        });
    }
    if chapters.is_empty() {
        return Err(AppError::invalid(
            if args.incomplete_policy == IncompletePolicy::TranslatedOnly {
                "noTranslatedChapters"
            } else {
                "selection"
            },
        ));
    }
    let metadata = super::book_metadata::read(&tx)?.filter(|m| m.current);
    let presentation = super::book_presentation::read(&tx)?;
    let cover = presentation
        .cover_asset_id
        .as_ref()
        .map(|id| -> Result<crate::export::fb2::Cover, AppError> {
            use base64::Engine;
            let (relative, mime, size): (String, String, u64) = tx
                .query_row(
                    "SELECT relative_path,mime,byte_length FROM assets WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(storage_error)?;
            let extension = match mime.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/gif" => "gif",
                "image/webp" => "webp",
                _ => return Err(AppError::invalid("assetMime")),
            };
            if relative != format!("assets/{id}.{extension}") {
                return Err(AppError::invalid("assetPath"));
            }
            let bytes = AssetStore::read_path(&directory.join(relative)).map_err(storage_error)?;
            if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(&bytes)) != *id {
                return Err(AppError::invalid("assetHash"));
            }
            Ok(crate::export::fb2::Cover {
                content_type: mime,
                base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            })
        })
        .transpose()?;

    Ok((
        chapters,
        OutputMeta {
            title: presentation
                .title
                .or_else(|| {
                    metadata
                        .as_ref()
                        .map(|m| m.title.clone())
                        .filter(|s| !s.trim().is_empty())
                })
                .or(presentation.source_title)
                .unwrap_or_default(),
            author: presentation
                .author
                .or_else(|| {
                    metadata
                        .as_ref()
                        .map(|m| m.author.clone())
                        .filter(|s| !s.trim().is_empty())
                })
                .or(presentation.source_author)
                .unwrap_or_default(),
            annotation: presentation
                .summary
                .or_else(|| metadata.map(|m| m.summary).filter(|s| !s.trim().is_empty()))
                .or(presentation.source_summary)
                .filter(|s| !s.is_empty()),
            cover,
            lang: settings.choices.target_language,
            images,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        contracts::EntitySelection,
        requests::{LanguagePair, ProjectChoices},
    };
    use std::io::Read;

    fn read_fb2_zip(path: &Path, name: &str) -> String {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        assert_eq!(archive.len(), 1);
        let mut entry = archive.by_name(name).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Deflated);
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn structured_exports_preserve_images_and_reject_incomplete_or_existing_outputs() {
        let temp = Scratch(
            std::env::temp_dir().join(format!("structured-export-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir(&temp.0).unwrap();
        let manager = ProjectManager::new(temp.0.join("app"));
        let preview = manager
            .inspect_source(
                ProjectKind::Book,
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/structural.epub"),
            )
            .unwrap();
        let project = manager
            .create(
                &preview.import_id.0,
                &ProjectChoices {
                    name: "Roundtrip".into(),
                    languages: LanguagePair {
                        source: Some("en".into()),
                        target: "ru".into(),
                    },
                    processing_profile_id: None,
                },
            )
            .unwrap();
        let mut args = BookExportArgs {
            project_id: project.id.clone(),
            selection: EntitySelection::All,
            destination: temp.0.join("output.epub").to_string_lossy().into_owned(),
            overwrite: false,
            format: BookExportFormat::Epub,
            incomplete_policy: IncompletePolicy::Reject,
        };
        assert!(export_book(&manager, &args).is_err());
        assert!(!Path::new(&args.destination).exists());
        args.incomplete_policy = IncompletePolicy::Originals;
        export_book(&manager, &args).unwrap();
        let before = std::fs::read(&args.destination).unwrap();
        assert!(export_book(&manager, &args).is_err());
        assert_eq!(std::fs::read(&args.destination).unwrap(), before);
        args.overwrite = true;
        args.incomplete_policy = IncompletePolicy::Reject;
        assert!(export_book(&manager, &args).is_err());
        assert_eq!(std::fs::read(&args.destination).unwrap(), before);
        args.incomplete_policy = IncompletePolicy::Originals;
        std::fs::write(&args.destination, b"Old export").unwrap();
        export_book(&manager, &args).unwrap();
        assert!(zip::ZipArchive::new(std::fs::File::open(&args.destination).unwrap()).is_ok());
        args.overwrite = false;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(before)).unwrap();
        let mut image_files = 0;
        let mut image_occurrences = 0;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            if entry.name().contains("images/") && entry.name().ends_with(".png") {
                image_files += 1;
            }
            if entry.name().ends_with(".xhtml") {
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                image_occurrences += text.matches("<img ").count();
            }
        }
        assert_eq!(image_files, 1);
        assert_eq!(image_occurrences, 3);
        args.format = BookExportFormat::Fb2;
        args.destination = temp.0.join("output.fb2.zip").to_string_lossy().into_owned();
        export_book(&manager, &args).unwrap();
        let text = read_fb2_zip(Path::new(&args.destination), "output.fb2");
        args.overwrite = true;
        std::fs::write(&args.destination, b"Old FB2 export").unwrap();
        export_book(&manager, &args).unwrap();
        assert_eq!(read_fb2_zip(Path::new(&args.destination), "output.fb2"), text);
        args.overwrite = false;
        assert_eq!(text.matches("<image ").count(), 3);
        assert_eq!(text.matches("<binary ").count(), 1);
        let imported = manager
            .inspect_source(ProjectKind::Book, Path::new(&args.destination))
            .unwrap();
        let imported = manager
            .create(
                &imported.import_id.0,
                &ProjectChoices {
                    name: "FB2 roundtrip".into(),
                    languages: LanguagePair {
                        source: Some("en".into()),
                        target: "ru".into(),
                    },
                    processing_profile_id: None,
                },
            )
            .unwrap();
        let roundtrip_path = temp.0.join("roundtrip.fb2.zip");
        export_book(
            &manager,
            &BookExportArgs {
                project_id: imported.id,
                selection: EntitySelection::All,
                destination: roundtrip_path.to_string_lossy().into_owned(),
                overwrite: false,
                format: BookExportFormat::Fb2,
                incomplete_policy: IncompletePolicy::Originals,
            },
        )
        .unwrap();
        let roundtrip = read_fb2_zip(&roundtrip_path, "roundtrip.fb2");
        assert_eq!(roundtrip.matches("<image ").count(), 3);
        assert_eq!(roundtrip.matches("<binary ").count(), 1);
        let before = crate::book::load::load_book_text(&text, "UTF-8").unwrap();
        let after = crate::book::load::load_book_text(&roundtrip, "UTF-8").unwrap();
        assert_eq!(before.chapters.len(), after.chapters.len());
        assert_eq!(
            before.blocks.iter().map(|c| &c.blocks).collect::<Vec<_>>(),
            after.blocks.iter().map(|c| &c.blocks).collect::<Vec<_>>()
        );
        let cover_path = temp.0.join("cover.png");
        image::RgbImage::new(3, 4).save(&cover_path).unwrap();
        manager
            .lease(&project.id)
            .unwrap()
            .with_connection(|db, directory| {
                let details = super::super::book_presentation::read(db)?;
                let details = super::super::book_presentation::update(
                    db,
                    &crate::app::requests::UpdateBookPresentationArgs {
                        project_id: project.id.clone(),
                        title: Some("Manual export title".into()),
                        author: Some("Manual author".into()),
                        summary: Some("Manual annotation".into()),
                        instructions: String::new(),
                        expected_revision: details.revision,
                    },
                )?;
                super::super::book_presentation::cover(
                    db,
                    directory,
                    cover_path.to_str(),
                    &details.revision,
                )?;
                Ok(())
            })
            .unwrap();
        args.destination = temp.0.join("covered.fb2").to_string_lossy().into_owned();
        export_book(&manager, &args).unwrap();
        assert!(!Path::new(&args.destination).exists());
        let covered = read_fb2_zip(
            Path::new(&format!("{}.zip", args.destination)),
            "covered.fb2",
        );
        assert!(covered.contains("Manual export title"));
        assert!(covered.contains("Manual annotation"));
        assert!(crate::book::load::load_book_text(&covered, "UTF-8")
            .unwrap()
            .cover
            .is_some());
        args.format = BookExportFormat::Txt;
        args.destination = temp.0.join("output.txt").to_string_lossy().into_owned();
        export_book(&manager, &args).unwrap();
        assert!(!std::fs::read_to_string(&args.destination)
            .unwrap()
            .contains("[[img:"));
        args.destination = temp
            .0
            .join("app/projects")
            .join(project.id.as_str())
            .join("bad.txt")
            .to_string_lossy()
            .into_owned();
        assert!(export_book(&manager, &args).is_err());
    }

    #[test]
    fn adopted_reference_exports_full_body_after_glossary_and_prompt_changes() {
        let temp = Scratch(
            std::env::temp_dir().join(format!("reference-export-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir(&temp.0).unwrap();
        let manager = ProjectManager::new(temp.0.join("app"));
        let preview = manager
            .inspect_source(
                ProjectKind::Book,
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/structural.epub"),
            )
            .unwrap();
        let project = manager
            .create(
                &preview.import_id.0,
                &ProjectChoices {
                    name: "Reference".into(),
                    languages: LanguagePair {
                        source: Some("en".into()),
                        target: "ru".into(),
                    },
                    processing_profile_id: None,
                },
            )
            .unwrap();
        let reference = temp.0.join("reference.fb2");
        let full = format!("{}КОНЕЦ ПОЛНОГО ПЕРЕВОДА", "Готовый перевод. ".repeat(2000));
        std::fs::write(&reference,format!(r#"<?xml version="1.0" encoding="utf-8"?><FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0"><description><title-info><book-title>Reference</book-title><lang>ru</lang></title-info></description><body><section><title><p>Chapter 1</p></title><p>{full}</p></section><section><title><p>Chapter 3</p></title><p>Последняя глава.</p></section></body></FictionBook>"#)).unwrap();
        super::super::book_reference::import(
            &manager,
            &crate::app::requests::BookReferenceImportArgs {
                project_id: project.id.clone(),
                path: reference.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        manager
            .lease(&project.id)
            .unwrap()
            .with_connection(|db, _| {
                db.execute("UPDATE glossary_state SET revision=revision+1", [])
                    .unwrap();
                db.execute("UPDATE project_settings SET revision=revision+1", [])
                    .unwrap();
                db.execute(
                    "UPDATE book_chapters SET instructions='New prompt',revision=revision+1",
                    [],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        let destination = temp.0.join("translated.txt");
        export_book(
            &manager,
            &BookExportArgs {
                project_id: project.id.clone(),
                selection: EntitySelection::All,
                destination: destination.to_string_lossy().into_owned(),
                overwrite: false,
                format: BookExportFormat::Txt,
                incomplete_policy: IncompletePolicy::Reject,
            },
        )
        .unwrap();
        let output = std::fs::read_to_string(destination).unwrap();
        assert!(output.contains(&full));
        assert!(output.contains("Последняя глава."));
        assert!(!output.contains("Original text."));
        manager.lease(&project.id).unwrap().with_connection(|db, _| {
            db.execute("UPDATE book_translations SET status='needs_review' WHERE chapter_id IN (SELECT id FROM book_chapters WHERE position=0)", []).unwrap();
            db.execute("UPDATE book_translations SET provenance='model',glossary_revision=0,settings_revision=0", []).unwrap();
            db.execute("INSERT INTO book_chapters(id,position,source_title) VALUES ('empty-volume',3,'第一卷 夜游神'),('later',4,'Untranslated later')", []).unwrap();
            // An untranslated chapter with an opening illustration must be skipped
            // entirely; only chapters with no source text qualify as image-only.
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,asset_id) SELECT 'later-image','later',0,'image',asset_id FROM book_source_blocks WHERE kind='image' LIMIT 1", []).unwrap();
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES ('later-text','later',1,'text','Later original text')", []).unwrap();
            db.execute("INSERT INTO book_chapters(id,position,source_title) VALUES ('later-illustration',5,'Future illustration')", []).unwrap();
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,asset_id) SELECT 'future-image','later-illustration',0,'image',asset_id FROM book_source_blocks WHERE kind='image' LIMIT 1", []).unwrap();
            Ok(())
        }).unwrap();
        let args = BookExportArgs {
            project_id: project.id.clone(),
            selection: EntitySelection::All,
            destination: temp
                .0
                .join("partial.fb2.zip")
                .to_string_lossy()
                .into_owned(),
            format: BookExportFormat::Fb2,
            incomplete_policy: IncompletePolicy::TranslatedOnly,
            overwrite: false,
        };
        export_book(&manager, &args).unwrap();
        let partial = read_fb2_zip(Path::new(&args.destination), "partial.fb2");
        assert!(partial.contains("Последняя глава."));
        assert!(partial.contains("КОНЕЦ ПОЛНОГО ПЕРЕВОДА"));
        assert!(!partial.contains("Later original text"));
        assert!(!partial.contains("Untranslated later"));
        assert!(!partial.contains("Future illustration"));
        assert!(!partial.contains("第一卷 夜游神"));
        assert_eq!(partial.matches("<image ").count(), 3);
        let originals_args = BookExportArgs {
            incomplete_policy: IncompletePolicy::Originals,
            destination: temp.0.join("with-originals.fb2.zip").to_string_lossy().into_owned(),
            ..args.clone()
        };
        export_book(&manager, &originals_args).unwrap();
        let originals = read_fb2_zip(Path::new(&originals_args.destination), "with-originals.fb2");
        assert!(originals.contains("КОНЕЦ ПОЛНОГО ПЕРЕВОДА"));
        assert!(originals.contains("Последняя глава."));
        assert!(originals.contains("Later original text"));
        assert!(originals.contains("Future illustration"));
        let empty_args = BookExportArgs {
            selection: EntitySelection::ExplicitIds {
                ids: vec!["later".into()],
            },
            destination: temp.0.join("empty.fb2.zip").to_string_lossy().into_owned(),
            ..args
        };
        assert!(export_book(&manager, &empty_args).is_err());
        assert!(!Path::new(&empty_args.destination).exists());
    }

    #[test]
    fn structural_text_is_never_interpreted_as_legacy_markup() {
        let chapters = vec![TranslatedChapter {
            index: 0,
            number: None,
            title: "Literal".into(),
            body: ChapterBody::Blocks(vec![ExportBlock::Text("[[img:ab12]]".into())]),
        }];
        let meta = OutputMeta::default();
        assert!(export::txt::render(&chapters).contains("[[img:ab12]]"));
        let xml = export::fb2::render(&chapters, &meta);
        assert!(xml.contains("<p>[[img:ab12]]</p>"));
        assert!(!xml.contains("<image "));
    }
}
