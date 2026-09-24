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
    translate_segments_with_glossary(provider,system,segments,&[]).await
}

pub(super) async fn translate_segments_with_glossary(
    provider: &dyn Provider, system: &str, segments: &[Segment], terms: &[shared::GlossaryTerm],
) -> Result<HashMap<String,String>,AppError> {
    transform_segments(provider, segments, |pending| Ok(Request::Structured {
        system: format!("{system}\nGlossary (respect pinned translations): {}",super::book_terms::payload(terms,&pending.iter().map(|s|s.text.as_str()).collect::<Vec<_>>().join("\n"),false)),
        user: serde_json::to_string(&serde_json::json!({"segments":pending})).map_err(|_| invalid_output())?,
    })).await
}

pub(super) async fn transform_segments(
    provider: &dyn Provider, segments: &[Segment], request: impl Fn(&[Segment]) -> Result<Request,AppError>,
) -> Result<HashMap<String,String>,AppError> {
    let mut accepted = HashMap::new();
    let mut pending = segments.to_vec();
    for _ in 0..3 {
        if pending.is_empty() {
            return Ok(accepted);
        }
        let response = provider.complete(request(&pending)?).await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.code == ErrorCode::InvalidOutput => continue,
            Err(error) => return Err(error),
        };
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
            // Discard an untrustworthy response and retry the unresolved input.
            continue;
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
    Retarget(super::book_retarget::RetargetOutput),
    Title {translation_id:String, revision:Revision, title:String, inputs:results::InputVersions},
    Translation(results::BookTranslation),
    Context(results::BookContext),
    Metadata(super::book_metadata::MetadataOutput),
    Glossary(super::book_glossary::GlossaryOutput),
}

fn digest(value: &impl Serialize) -> Result<String, AppError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| AppError::invalid("fingerprint"))?)
    ))
}
#[derive(Debug, Serialize)]
struct Continuity {
    id: Option<String>,
    summary: String,
    previous_tail: String,
}
fn predecessor(db: &rusqlite::Connection, chapter: &str) -> Result<Option<Continuity>, AppError> {
    let previous: Option<(String, Option<String>)> = db.query_row(
        "SELECT t.id,ctx.id FROM book_translations t JOIN book_chapters c ON c.id=t.chapter_id LEFT JOIN book_contexts ctx ON ctx.translation_id=t.id WHERE t.status IN ('ready','needs_review') AND t.target_language=(SELECT target_language FROM project_settings WHERE singleton=1) AND c.position=(SELECT MAX(position) FROM book_chapters WHERE position<(SELECT position FROM book_chapters WHERE id=?1) AND EXISTS(SELECT 1 FROM book_source_blocks b WHERE b.chapter_id=book_chapters.id AND b.kind IN ('text','caption') AND trim(b.text)!='')) ORDER BY t.revision DESC LIMIT 1",[chapter],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?;
    let Some((translation,id))=previous else {return Ok(None)};
    let mut q=db.prepare("SELECT translated_text FROM book_translation_blocks JOIN book_source_blocks b ON b.id=source_block_id WHERE translation_id=?1 ORDER BY b.position").map_err(storage_error)?;
    let text=q.query_map([&translation],|r|r.get::<_,String>(0)).map_err(storage_error)?.collect::<Result<Vec<_>,_>>().map_err(storage_error)?.into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join("\n\n");
    let summary: Option<String> = db.query_row("SELECT ctx.summary FROM book_contexts ctx JOIN book_translations t ON t.id=ctx.translation_id JOIN book_chapters c ON c.id=t.chapter_id WHERE c.position<(SELECT position FROM book_chapters WHERE id=?1) AND t.status IN ('ready','needs_review') AND t.target_language=(SELECT target_language FROM project_settings WHERE singleton=1) AND trim(ctx.summary)!='' AND t.revision=(SELECT MAX(revision) FROM book_translations WHERE chapter_id=c.id AND target_language=t.target_language) ORDER BY c.position DESC LIMIT 1",[chapter],|r|r.get(0)).optional().map_err(storage_error)?;
    Ok(Some(Continuity {id,summary:summary.unwrap_or_default(),previous_tail:text.chars().rev().take(1200).collect::<String>().chars().rev().collect()}))
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
        if stage == "retarget" {
            return lease.with_connection(|db, _| super::book_retarget::fingerprint(db, run, entity));
        }
        if stage == "glossary" {
            return lease.with_connection(|db, _| {
                super::book_glossary::fingerprint(db, entity, run, self.provider.as_ref())
            });
        }
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
                "retarget" => super::book_retarget::compute(lease, run, entity, self.provider.as_ref()).await.map(BookOutput::Retarget),
                "translation" => self.translate(lease, run, entity).await,
                "title" => self.translate_title(lease, run, entity).await,
                "context" => self.context(lease, entity).await,
                "glossary" => {
                    super::book_glossary::compute(lease, run, entity, self.provider.as_ref())
                        .await
                        .map(BookOutput::Glossary)
                }
                "metadata" => super::book_metadata::compute(lease, run, self.provider.as_ref())
                    .await
                    .map(BookOutput::Metadata),
                _ => Err(AppError::invalid("bookStage")),
            }
        })
    }
    fn steps(&self, run: &runs::RunRecord) -> Vec<(String, String)> {
        let mut steps = Vec::new();
        if run.kind == "book_translation" && run.snapshot.stages.iter().any(|s| s == "glossary") {
            steps.extend(
                run.snapshot
                    .selected_ids
                    .iter()
                    .map(|id| (id.clone(), "glossary".into())),
            );
        }
        for id in &run.snapshot.selected_ids {
            for stage in &run.snapshot.stages {
                if run.kind == "book_translation" && stage == "glossary" {
                    continue;
                }
                steps.push((id.clone(), stage.clone()));
            }
        }
        steps
    }
    fn after_persist(
        &self,
        tx: &Transaction<'_>,
        run: &runs::RunRecord,
        stage: &str,
    ) -> Result<(), AppError> {
        if run.kind == "book_translation" && stage == "glossary" {
            let mut snapshot = run.snapshot.clone();
            snapshot.glossary_revision = shared::glossary_revision(tx)?;
            let json =
                serde_json::to_string(&snapshot).map_err(|_| AppError::invalid("jobSnapshot"))?;
            tx.execute(
                "UPDATE job_runs SET settings_snapshot=?1,revision=revision+1 WHERE id=?2",
                rusqlite::params![json, run.id],
            )
            .map_err(storage_error)?;
        }
        Ok(())
    }
    fn persist(&self, tx: &Transaction<'_>, output: BookOutput) -> Result<String, AppError> {
        match output {
            BookOutput::Retarget(value) => super::book_retarget::persist(tx, value),
            BookOutput::Title {translation_id,revision,title,inputs} => results::edit_translation_title_in(tx,&translation_id,&revision,&title,Some(&inputs)).map(|value|value.0),
            BookOutput::Glossary(value) => super::book_glossary::persist(tx, value),
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
    async fn translate_title(&self,lease:&ProjectLease,run:&runs::RunRecord,entity:&str)->Result<BookOutput,AppError> {
        let (chapter,glossary)=lease.with_connection(|db,_| {
            if shared::settings(db)?.revision!=run.snapshot.settings_revision || shared::glossary_revision(db)?!=run.snapshot.glossary_revision {return Err(crate::storage::repository::conflict())}
            Ok((ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?.chapter(entity)?,shared::glossary(db)?))
        })?;
        let translation=chapter.translation.ok_or_else(||AppError::invalid("noTranslation"))?;
        let target=&run.snapshot.settings.target_language;
        let system=format!("Translate only the supplied chapter title from {} to {target}. Return JSON {{\"segments\":[{{\"id\":\"exact input id\",\"text\":\"translated title\"}}]}}. Keep the title's chapter number. Book instructions: {}. Chapter instructions: {}",run.snapshot.settings.source_language.as_deref().unwrap_or("the source language"),self.instructions.as_deref().unwrap_or(""),chapter.instructions);
        let segments=split_segments(&format!("{entity}:title"),&chapter.chapter.title,2048)?;
        let translated=translate_segments_with_glossary(self.provider.as_ref(),&system,&segments,&glossary).await?;
        let mut title=segments.iter().map(|s|translated.get(&s.id).cloned().ok_or_else(invalid_output)).collect::<Result<Vec<_>,_>>()?.join("");
        super::book_language::repair(self.provider.as_ref(),target,&chapter.chapter.title,&mut title,&mut [],&glossary).await;
        Ok(BookOutput::Title {translation_id:translation.id,revision:translation.revision,title,inputs:results::InputVersions{source:chapter.chapter.revision,settings:run.snapshot.settings_revision.clone(),glossary:run.snapshot.glossary_revision.clone()}})
    }
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
        let system=format!("Translate every text segment faithfully from {source} to {target}. Return only JSON {{\"segments\":[{{\"id\":\"exact input id\",\"text\":\"translation\"}}]}}. Keep every ID exactly once. Do not add image markers or explanations. Segments may continue mid-paragraph. Preserve whitespace boundaries. Instructions: {}\nChapter instructions: {}\nPrevious context: {}\nReference: {}",self.instructions.as_deref().unwrap_or(""),chapter.instructions,serde_json::to_string(&context).map_err(|_|invalid_output())?,reference.as_deref().unwrap_or(""));
        let mut translated = HashMap::new();
        for batch in segments.chunks(4) {
            translated.extend(translate_segments_with_glossary(self.provider.as_ref(), &system, batch, &glossary).await?);
        }
        let mut title = segments
            .iter()
            .take_while(|s| s.id.starts_with(&format!("{entity}:title:")))
            .map(|s| translated.get(&s.id).cloned().ok_or_else(invalid_output))
            .collect::<Result<Vec<_>, _>>()?
            .join("");
        let mut blocks = layout
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
        let source_text = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n");
        super::book_language::repair(
            self.provider.as_ref(), target, &source_text, &mut title, &mut blocks,
            &glossary,
        ).await;
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
        let (id,revision,text,previous)=lease.with_connection(|db,_|{
            let (id,revision):(String,i64)=db.query_row("SELECT id,revision FROM book_translations WHERE chapter_id=?1 AND status='ready' ORDER BY revision DESC LIMIT 1",[entity],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage_error)?;
            let mut query=db.prepare("SELECT translated_text FROM book_translation_blocks JOIN book_source_blocks ON book_source_blocks.id=source_block_id WHERE translation_id=?1 ORDER BY position").map_err(storage_error)?;
            let rows=query.query_map([&id],|r|r.get::<_,String>(0)).map_err(storage_error)?;let text=rows.collect::<Result<Vec<_>,_>>().map_err(storage_error)?.join("\n\n");
            Ok((id,revision,text,predecessor(db,entity)?))
        })?;
        let response=self.provider.complete(Request::Structured{system:"Update the rolling story summary using the previous summary and the newly translated chapter. Return JSON {\"summary\":\"concise cumulative continuity notes\"}. Preserve relevant earlier facts, names and unresolved events; integrate new events. Use the chapter's language. Keep the summary under 2000 characters.".into(),user:serde_json::json!({"previousSummary":previous.as_ref().map(|c|c.summary.as_str()).unwrap_or(""),"chapter":text}).to_string()}).await?;
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
            predecessor_id: previous.and_then(|c|c.id),
        }))
    }
}

#[cfg(test)]
mod continuity_tests {
    use super::*;
    #[test]
    fn reference_tail_uses_full_saved_text_and_keeps_earlier_rolling_summary() {
        let mut db=crate::storage::tests::database(crate::app::contracts::ProjectKind::Book);
        for (position,id) in ["first","reference","next"].iter().enumerate() {
            db.execute("INSERT INTO book_chapters(id,position,source_title) VALUES(?1,?2,'Title')",rusqlite::params![id,position]).unwrap();
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES(?1,?1,0,'text','Source')",[id]).unwrap();
        }
        let reference=format!("{}ФИНАЛ РЕФЕРЕНСА", "Полный текст. ".repeat(500));
        for (chapter,body,origin) in [("first","Первая глава","model"),("reference",reference.as_str(),"reference")] {
            results::save_translation(&mut db,&results::BookTranslation {
                id:chapter.into(),chapter_id:chapter.into(),inputs:results::InputVersions{source:Revision("0".into()),settings:Revision("0".into()),glossary:Revision("0".into())},expected_translation:None,title:"Глава".into(),provenance:origin.into(),context_fingerprint:"test".into(),blocks:vec![(chapter.into(),body.into())],
            }).unwrap();
            results::save_context(&db,&results::BookContext{id:format!("ctx-{chapter}"),translation_id:chapter.into(),translation_revision:Revision("0".into()),summary:if chapter=="first" {"Накопленное саммари".into()} else {String::new()},previous_tail:"Obsolete cached tail".into(),predecessor_id:None}).unwrap();
        }
        let context=predecessor(&db,"next").unwrap().unwrap();
        assert_eq!(context.summary,"Накопленное саммари");
        assert_eq!(context.previous_tail.chars().count(),1200);
        assert!(context.previous_tail.ends_with("ФИНАЛ РЕФЕРЕНСА"));
        assert!(reference.ends_with(&context.previous_tail));
        db.execute("DELETE FROM book_contexts WHERE translation_id='reference'",[]).unwrap();
        let restored=predecessor(&db,"next").unwrap().unwrap();
        assert_eq!(restored.previous_tail,context.previous_tail);
        assert_eq!(restored.summary,context.summary);
        assert!(restored.id.is_none());
        assert!(predecessor(&db,"first").unwrap().is_none());
    }
}
