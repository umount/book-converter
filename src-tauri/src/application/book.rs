//! Structural book translation: stable text segment IDs, immutable image positions.
use crate::{
    ai::{Provider, Request},
    app::contracts::{AppError, BookBlockContent, ErrorCode, Revision},
    jobs::durable::StepExecutor,
    project::lifecycle::ProjectLease,
    storage::{
        repository::{storage_error, ProjectRepository},
        results, runs, shared,
    },
};
use rusqlite::{OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub text: String,
}

pub fn split_segments(id: &str, text: &str, limit: usize) -> Result<Vec<Segment>, AppError> {
    if limit == 0 {
        return Err(AppError::invalid("segmentLimit"));
    }
    if text.is_empty() {
        return Ok(vec![Segment {
            id: format!("{id}:0"),
            text: String::new(),
        }]);
    }
    let mut result = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    for (at, _) in text.char_indices() {
        if chars == limit {
            result.push(Segment {
                id: format!("{id}:{}", result.len()),
                text: text[start..at].into(),
            });
            start = at;
            chars = 0;
        }
        chars += 1;
    }
    result.push(Segment {
        id: format!("{id}:{}", result.len()),
        text: text[start..].into(),
    });
    Ok(result)
}

fn invalid_output() -> AppError {
    AppError {
        code: ErrorCode::InvalidOutput,
        message_key: "errors.translationSegments".into(),
        params: Default::default(),
        retryable: false,
    }
}

/// Keep valid segments and ask again only for missing/duplicated ones, with a finite repair budget.
pub async fn translate_segments(
    provider: &dyn Provider,
    system: &str,
    segments: &[Segment],
) -> Result<HashMap<String, String>, AppError> {
    let mut accepted = HashMap::new();
    let mut pending = segments.to_vec();
    for _ in 0..3 {
        if pending.is_empty() {
            return Ok(accepted);
        }
        let response = provider
            .complete(Request::Structured {
                system: system.into(),
                user: serde_json::to_string(&serde_json::json!({"segments":pending}))
                    .map_err(|_| invalid_output())?,
            })
            .await?;
        if response.finish_reason != "stop" {
            if response.finish_reason == "length" {
                continue;
            }
            return Err(invalid_output());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Output {
            segments: Vec<Segment>,
        }
        let Ok(output) = serde_json::from_str::<Output>(&response.text) else {
            continue;
        };
        let expected: HashSet<_> = pending.iter().map(|s| s.id.as_str()).collect();
        if output
            .segments
            .iter()
            .any(|s| !expected.contains(s.id.as_str()))
        {
            return Err(invalid_output());
        }
        let mut counts = HashMap::new();
        for s in &output.segments {
            *counts.entry(s.id.as_str()).or_insert(0) += 1;
        }
        for s in &output.segments {
            if counts.get(s.id.as_str()) == Some(&1)
                && (!s.text.trim().is_empty()
                    || pending
                        .iter()
                        .find(|p| p.id == s.id)
                        .is_some_and(|p| p.text.trim().is_empty()))
            {
                accepted.insert(s.id.clone(), s.text.clone());
            }
        }
        pending.retain(|s| !accepted.contains_key(&s.id));
    }
    if pending.is_empty() {
        Ok(accepted)
    } else {
        Err(invalid_output())
    }
}

pub struct BookPipeline {
    pub provider: Arc<dyn Provider>,
    pub instructions: Option<String>,
}
pub enum BookOutput {
    Translation(results::BookTranslation),
    Context(results::BookContext),
    Metadata(super::book_metadata::MetadataOutput),
}

fn digest(value: &impl Serialize) -> Result<String, AppError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| AppError::invalid("fingerprint"))?)
    ))
}
fn predecessor(
    db: &rusqlite::Connection,
    chapter: &str,
) -> Result<Option<(String, String)>, AppError> {
    db.query_row("SELECT ctx.id,ctx.summary || char(10) || ctx.previous_tail FROM book_contexts ctx JOIN book_translations t ON t.id=ctx.translation_id JOIN book_chapters c ON c.id=t.chapter_id WHERE t.status='ready' AND t.source_revision=c.revision AND c.position=(SELECT MAX(position) FROM book_chapters WHERE position<(SELECT position FROM book_chapters WHERE id=?1)) AND t.revision=(SELECT MAX(revision) FROM book_translations WHERE chapter_id=c.id AND target_language=t.target_language) ORDER BY t.revision DESC LIMIT 1",[chapter],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)
}

impl StepExecutor for BookPipeline {
    type Output = BookOutput;
    fn entity_kind(&self) -> &'static str {
        "chapter"
    }
    fn fingerprint(
        &self,
        lease: &ProjectLease,
        run: &runs::RunRecord,
        entity: &str,
        stage: &str,
    ) -> Result<String, AppError> {
        if stage == "metadata" {
            return lease.with_connection(|db, _| {
                digest(&(
                    super::book_metadata::fingerprint(db)?,
                    &run.snapshot.prompt_version,
                    self.provider.profile(),
                ))
            });
        }
        lease.with_connection(|db,_|{
            let source:i64=db.query_row("SELECT revision FROM book_chapters WHERE id=?1",[entity],|r|r.get(0)).map_err(storage_error)?;
            let current_settings=shared::settings(db)?.revision;let glossary=shared::glossary_revision(db)?;
            let translation:Option<String>=if stage=="context"{db.query_row("SELECT id FROM book_translations WHERE chapter_id=?1 AND status='ready' ORDER BY revision DESC LIMIT 1",[entity],|r|r.get(0)).optional().map_err(storage_error)?}else{None};
            digest(&(entity,stage,source,current_settings.0,glossary.0,&run.snapshot.prompt_version,&self.instructions,self.provider.profile(),predecessor(db,entity)?,translation))
        })
    }
    fn compute<'a>(
        &'a self,
        lease: &'a ProjectLease,
        run: &'a runs::RunRecord,
        entity: &'a str,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<BookOutput, AppError>> + Send + 'a>> {
        Box::pin(async move {
            match stage {
                "translation" => self.translate(lease, run, entity).await,
                "context" => self.context(lease, entity).await,
                "metadata" => super::book_metadata::compute(lease, run, self.provider.as_ref())
                    .await
                    .map(BookOutput::Metadata),
                _ => Err(AppError::invalid("bookStage")),
            }
        })
    }
    fn persist(&self, tx: &Transaction<'_>, output: BookOutput) -> Result<String, AppError> {
        match output {
            BookOutput::Metadata(value) => super::book_metadata::persist(tx, value),
            BookOutput::Translation(value) => {
                results::save_translation_in(tx, &value)?;
                Ok(value.id)
            }
            BookOutput::Context(value) => {
                results::save_context(tx, &value)?;
                Ok(value.id)
            }
        }
    }
}

impl BookPipeline {
    async fn translate(
        &self,
        lease: &ProjectLease,
        run: &runs::RunRecord,
        entity: &str,
    ) -> Result<BookOutput, AppError> {
        let (chapter,inputs,prior,glossary,context,reference)=lease.with_connection(|db,_|{
            let settings=shared::settings(db)?;
            if settings.revision!=run.snapshot.settings_revision{return Err(AppError::invalid("jobSettingsChanged"));}
            let glossary_revision=shared::glossary_revision(db)?;
            // Glossary versions stay fixed during this run; extraction is a separate job.
            if glossary_revision!=run.snapshot.glossary_revision{return Err(AppError::invalid("jobGlossaryChanged"));}
            let chapter=ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?.chapter(entity)?;
            let prior:Option<i64>=db.query_row("SELECT MAX(revision) FROM book_translations WHERE chapter_id=?1 AND target_language=?2",rusqlite::params![entity,settings.choices.target_language],|r|r.get(0)).map_err(storage_error)?;
            let terms=shared::glossary(db)?;
            let reference:Option<String>=db.query_row("SELECT text FROM book_reference_chapters JOIN book_reference_mappings ON reference_id=id WHERE chapter_id=?1",[entity],|r|r.get(0)).optional().map_err(storage_error)?;
            let inputs=results::InputVersions{source:chapter.chapter.revision.clone(),settings:settings.revision,glossary:glossary_revision};
            Ok((chapter,inputs,prior,terms,predecessor(db,entity)?,reference))
        })?;
        let mut segments =
            split_segments(&format!("{entity}:title"), &chapter.chapter.title, 2048)?;
        let mut layout = Vec::new();
        for block in &chapter.blocks {
            if let BookBlockContent::Text { text } | BookBlockContent::Caption { text } =
                &block.content
            {
                let parts = split_segments(&block.id, text, 2048)?;
                layout.push((
                    block.id.clone(),
                    parts.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
                ));
                segments.extend(parts);
            }
        }
        if layout.is_empty() {
            return Err(AppError::invalid("noTextBlocks"));
        }
        let target = &run.snapshot.settings.target_language;
        let source = run
            .snapshot
            .settings
            .source_language
            .as_deref()
            .unwrap_or("the detected source language");
        let system=format!("Translate every text segment faithfully from {source} to {target}. Return only JSON {{\"segments\":[{{\"id\":\"exact input id\",\"text\":\"translation\"}}]}}. Keep every ID exactly once. Do not add image markers or explanations. Segments may continue mid-paragraph. Preserve whitespace boundaries. Instructions: {}\nChapter instructions: {}\nGlossary (respect pinned translations): {}\nPrevious context: {}\nReference: {}",self.instructions.as_deref().unwrap_or(""),chapter.instructions,serde_json::to_string(&glossary).map_err(|_|invalid_output())?,context.as_ref().map(|c|c.1.as_str()).unwrap_or(""),reference.as_deref().unwrap_or(""));
        let mut translated = HashMap::new();
        for batch in segments.chunks(4) {
            translated.extend(translate_segments(self.provider.as_ref(), &system, batch).await?);
        }
        let title = segments
            .iter()
            .take_while(|s| s.id.starts_with(&format!("{entity}:title:")))
            .map(|s| translated.get(&s.id).cloned().ok_or_else(invalid_output))
            .collect::<Result<Vec<_>, _>>()?
            .join("");
        let blocks = layout
            .into_iter()
            .map(|(id, parts)| {
                Ok((
                    id,
                    parts
                        .iter()
                        .map(|id| translated.get(id).cloned().ok_or_else(invalid_output))
                        .collect::<Result<Vec<_>, AppError>>()?
                        .join(""),
                ))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        Ok(BookOutput::Translation(results::BookTranslation {
            id: uuid::Uuid::new_v4().to_string(),
            chapter_id: entity.into(),
            inputs,
            expected_translation: prior.map(|v| Revision(v.to_string())),
            title,
            provenance: format!(
                "{}:{}",
                self.provider.profile().id,
                self.provider.profile().model
            ),
            context_fingerprint: digest(&context)?,
            blocks,
        }))
    }
    async fn context(&self, lease: &ProjectLease, entity: &str) -> Result<BookOutput, AppError> {
        let (id,revision,text,predecessor_id)=lease.with_connection(|db,_|{
            let (id,revision):(String,i64)=db.query_row("SELECT id,revision FROM book_translations WHERE chapter_id=?1 AND status='ready' ORDER BY revision DESC LIMIT 1",[entity],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage_error)?;
            let mut query=db.prepare("SELECT translated_text FROM book_translation_blocks JOIN book_source_blocks ON book_source_blocks.id=source_block_id WHERE translation_id=?1 ORDER BY position").map_err(storage_error)?;
            let rows=query.query_map([&id],|r|r.get::<_,String>(0)).map_err(storage_error)?;let text=rows.collect::<Result<Vec<_>,_>>().map_err(storage_error)?.join("\n\n");
            Ok((id,revision,text,predecessor(db,entity)?.map(|c|c.0)))
        })?;
        let response=self.provider.complete(Request::Structured{system:"Return JSON {\"summary\":\"concise continuity notes\"} for the translated chapter. Preserve names and unresolved events. Use the chapter's language.".into(),user:text.clone()}).await?;
        if response.finish_reason != "stop" {
            return Err(invalid_output());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Summary {
            summary: String,
        }
        let summary: Summary =
            serde_json::from_str(&response.text).map_err(|_| invalid_output())?;
        let tail = text
            .chars()
            .rev()
            .take(1200)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        Ok(BookOutput::Context(results::BookContext {
            id: uuid::Uuid::new_v4().to_string(),
            translation_id: id,
            translation_revision: Revision(revision.to_string()),
            summary: summary.summary,
            previous_tail: tail,
            predecessor_id,
        }))
    }
}
