//! Version-1 domain request schemas. Defining a DTO does not register a command.
//! Paths are native paths, page geometry is in canonical pixels, revisions are decimal strings.
use super::contracts::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanguagePair {
    pub source: Option<String>,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectChoices {
    pub name: String,
    pub languages: LanguagePair,
    pub processing_profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectSourceArgs {
    pub kind: ProjectKind,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateProjectArgs {
    pub import_id: ImportId,
    pub choices: ProjectChoices,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSessionArgs {
    pub import_id: ImportId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectArgs {
    pub project_id: ProjectId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveExportArgs {
    pub project_id: ProjectId,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveImportArgs {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListChaptersArgs {
    pub project_id: ProjectId,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetChapterArgs {
    pub project_id: ProjectId,
    pub chapter_id: ChapterId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateBookBlockArgs {
    pub project_id: ProjectId,
    pub block_id: BlockId,
    pub text: String,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateChapterInstructionsArgs {
    pub project_id: ProjectId,
    pub chapter_id: ChapterId,
    pub instructions: String,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranslationOptions {
    // Maximum eligible chapters in this run; applied after skipping existing translations.
    pub max_chapters: u32,
    pub extract_glossary: bool,
    pub force: bool,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartBookTranslationArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub options: TranslationOptions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookReferenceImportArgs {
    pub project_id: ProjectId,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadataView {
    pub id: String,
    pub title: String,
    pub author: String,
    pub summary: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceChapterView {
    pub id: String,
    pub position: u32,
    pub title: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceMapping {
    pub chapter_id: ChapterId,
    pub reference_id: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookReferenceView {
    pub fingerprint: String,
    pub chapters: Vec<ReferenceChapterView>,
    pub mappings: Vec<ReferenceMapping>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookReferenceMapArgs {
    pub project_id: ProjectId,
    pub expected_fingerprint: String,
    pub mappings: Vec<ReferenceMapping>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookReplacePreviewArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub search: String,
    pub replacement: String,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookReplaceApplyArgs {
    pub project_id: ProjectId,
    pub preview_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookReplaceChange {
    pub chapter_id: ChapterId,
    pub block_id: BlockId,
    pub before: String,
    pub after: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookReplacePreview {
    pub preview_id: String,
    pub changes: Vec<BookReplaceChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookExportArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub destination: String,
    pub format: BookExportFormat,
    pub incomplete_policy: IncompletePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListMangaPagesArgs {
    pub project_id: ProjectId,
    pub volume_id: Option<VolumeId>,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MangaVolumeSummary {
    pub id: VolumeId,
    pub title: String,
    pub reading_direction: String,
    pub page_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetMangaPageArgs {
    pub project_id: ProjectId,
    pub page_id: PageId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMangaRegionArgs {
    pub project_id: ProjectId,
    pub region_id: RegionId,
    pub patch: RegionPatch,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMangaMaskArgs {
    pub project_id: ProjectId,
    pub page_id: PageId,
    pub region_id: Option<RegionId>,
    pub asset_id: AssetId,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMangaOrderArgs {
    pub project_id: ProjectId,
    pub page_id: PageId,
    pub region_ids: Vec<RegionId>,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MangaStageOptions {
    pub max_pages: u32,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartMangaStageArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub stage: MangaStage,
    pub options: MangaStageOptions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewMangaPageArgs {
    pub project_id: ProjectId,
    pub page_id: PageId,
    pub result_revision: Revision,
    pub decision: ReviewDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MangaExportArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub destination: String,
    pub format: MangaExportFormat,
    pub incomplete_policy: IncompletePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobArgs {
    pub project_id: ProjectId,
    pub job_id: JobId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListJobsArgs {
    pub project_id: ProjectId,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BookExportFormat {
    Txt,
    Fb2,
    Epub,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MangaExportFormat {
    Cbz,
    Pdf,
    Images,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IncompletePolicy {
    Reject,
    Originals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Unreviewed,
    NeedsReview,
    Approved,
}

/// Each edit targets one revision domain, avoiding ambiguous partial updates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegionPatch {
    SourceText { text: String },
    TranslatedText { text: String },
    Bounds { bounds: PixelBounds },
}

impl Revision {
    pub fn value(&self) -> Result<i64, AppError> {
        let value = self
            .0
            .parse::<i64>()
            .map_err(|_| AppError::invalid("revision"))?;
        if value < 0 || value.to_string() != self.0 {
            return Err(AppError::invalid("revision"));
        }
        Ok(value)
    }
}

impl EntitySelection {
    /// Freeze stable IDs at run creation; a range is inclusive in current order.
    pub fn resolve(&self, ordered_ids: &[String]) -> Result<Vec<String>, AppError> {
        let known: std::collections::HashSet<_> = ordered_ids.iter().collect();
        if known.len() != ordered_ids.len() || ordered_ids.iter().any(|id| id.is_empty()) {
            return Err(AppError::invalid("entityOrder"));
        }
        match self {
            Self::All => Ok(ordered_ids.to_vec()),
            Self::ExplicitIds { ids } => {
                let requested: std::collections::HashSet<_> = ids.iter().collect();
                if ids.is_empty() || requested.len() != ids.len() || !requested.is_subset(&known) {
                    return Err(AppError::invalid("selection"));
                }
                Ok(ordered_ids
                    .iter()
                    .filter(|id| requested.contains(id))
                    .cloned()
                    .collect())
            }
            Self::Range { first, last } => {
                let start = ordered_ids.iter().position(|id| id == first);
                let end = ordered_ids.iter().position(|id| id == last);
                match (start, end) {
                    (Some(start), Some(end)) if start <= end => {
                        Ok(ordered_ids[start..=end].to_vec())
                    }
                    _ => Err(AppError::invalid("selection")),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlossaryListArgs {
    pub project_id: ProjectId,
    pub query: String,
    pub pinned_only: bool,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessingChoices {
    pub book_translation_profile: Option<String>,
    pub manga_recognition_profile: Option<String>,
    pub manga_translation_profile: Option<String>,
    pub assistant_profile: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettingsView {
    pub languages: LanguagePair,
    pub choices: ProcessingChoices,
    pub revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryTermView {
    pub id: TermId,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub pinned: bool,
    pub frequency: u32,
    pub revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryPage {
    pub total: u32,
    pub items: Vec<GlossaryTermView>,
    pub next_cursor: Option<String>,
    pub revision: Revision,
    pub settings_revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartBookGlossaryArgs {
    pub project_id: ProjectId,
    pub selection: EntitySelection,
    pub max_chapters: u32,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlossaryPutArgs {
    pub project_id: ProjectId,
    pub term_id: TermId,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub pinned: bool,
    pub expected_revision: Option<Revision>,
    pub expected_settings_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlossaryDeleteArgs {
    pub project_id: ProjectId,
    pub term_id: TermId,
    pub expected_revision: Revision,
    pub expected_settings_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantSendArgs {
    pub project_id: ProjectId,
    pub chapter_id: Option<ChapterId>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantConfirmArgs {
    pub project_id: ProjectId,
    pub proposal_id: String,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSettingsUpdateArgs {
    pub project_id: ProjectId,
    pub choices: ProcessingChoices,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectManifestArgs {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChapterSummary {
    pub status: String,
    pub origin: Option<String>,
    pub needs_review: bool,
    pub id: ChapterId,
    pub position: u32,
    pub title: String,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChapterPage {
    pub items: Vec<ChapterSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookChapterView {
    pub status: String,
    pub lang_issues: Vec<String>,
    pub translation_error: Option<AppError>,
    pub instructions: String,
    pub chapter: ChapterSummary,
    pub blocks: Vec<BookBlockView>,
    pub translation: Option<TranslationSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageSummary {
    pub id: PageId,
    pub volume_id: VolumeId,
    pub position: u32,
    pub original_asset_id: AssetId,
    pub thumbnail_asset_id: Option<AssetId>,
    pub width: u32,
    pub height: u32,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageSummaryPage {
    pub items: Vec<PageSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MangaRegionView {
    pub id: RegionId,
    pub page_id: PageId,
    pub reading_order: u32,
    pub bounds: PixelBounds,
    pub source_text: String,
    pub translated_text: Option<String>,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MangaPageView {
    pub page: PageSummary,
    pub regions: Vec<MangaRegionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportPreview {
    pub import_id: ImportId,
    pub kind: ProjectKind,
    pub source: SourceDescriptor,
    pub suggested_name: String,
    pub detected_language: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSummary {
    pub descriptor: ProjectDescriptor,
    pub progress: DomainProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DomainProgress {
    Book {
        chapters: u32,
        translated: u32,
    },
    Manga {
        pages: u32,
        lettered: u32,
        approved: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSummary {
    pub origin: String,
    pub id: String,
    pub revision: Revision,
    pub title: String,
    pub status: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub job: JobRef,
    pub kind: String,
    pub state: JobState,
    pub revision: Revision,
    pub total_steps: u32,
    pub completed_steps: u32,
    pub remaining_seconds: Option<u32>,
    pub error: Option<AppError>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTranslationBlockArgs {
    pub project_id: ProjectId,
    pub translation_id: String,
    pub block_id: BlockId,
    pub text: String,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTranslationTitleArgs {
    pub project_id: ProjectId,
    pub translation_id: String,
    pub expected_revision: Revision,
    pub title: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartBookTitleArgs {
    pub project_id: ProjectId,
    pub chapter_id: ChapterId,
    pub expected_revision: Revision,
}

pub fn typescript() -> String {
    let config = ts_rs::Config::default();
    let declarations = [
        TranslationSummary::decl(&config),
        JobView::decl(&config),
        UpdateTranslationBlockArgs::decl(&config),
        UpdateTranslationTitleArgs::decl(&config),
        StartBookTitleArgs::decl(&config),
        StartBookRetargetArgs::decl(&config),
        BookRetargetPreview::decl(&config),
        GlossaryListArgs::decl(&config),
        ProcessingChoices::decl(&config),
        ProjectSettingsView::decl(&config),
        GlossaryTermView::decl(&config),
        GlossaryPage::decl(&config),
        StartBookGlossaryArgs::decl(&config),
        GlossaryPutArgs::decl(&config),
        GlossaryDeleteArgs::decl(&config),
        AssistantSendArgs::decl(&config),
        AssistantMessage::decl(&config),
        AssistantProposal::decl(&config),
        AssistantView::decl(&config),
        ProviderEntry::decl(&config),
        SaveProviderArgs::decl(&config),
        BookSearchSide::decl(&config),
        BookSearchArgs::decl(&config),
        BookSearchMatch::decl(&config),
        BookSearchPage::decl(&config),
        AssistantConfirmArgs::decl(&config),
        ProjectSettingsUpdateArgs::decl(&config),
        InspectManifestArgs::decl(&config),
        ChapterSummary::decl(&config),
        ChapterPage::decl(&config),
        BookChapterView::decl(&config),
        MangaVolumeSummary::decl(&config),
        PageSummary::decl(&config),
        PageSummaryPage::decl(&config),
        MangaRegionView::decl(&config),
        MangaPageView::decl(&config),
        ImportPreview::decl(&config),
        ProjectSummary::decl(&config),
        DomainProgress::decl(&config),
        LanguagePair::decl(&config),
        ProjectChoices::decl(&config),
        InspectSourceArgs::decl(&config),
        CreateProjectArgs::decl(&config),
        ImportSessionArgs::decl(&config),
        ProjectArgs::decl(&config),
        ArchiveExportArgs::decl(&config),
        ArchiveImportArgs::decl(&config),
        ListChaptersArgs::decl(&config),
        GetChapterArgs::decl(&config),
        UpdateBookBlockArgs::decl(&config),
        UpdateChapterInstructionsArgs::decl(&config),
        TranslationOptions::decl(&config),
        BookPresentation::decl(&config),
        UpdateBookPresentationArgs::decl(&config),
        SetBookCoverArgs::decl(&config),
        StartBookTranslationArgs::decl(&config),
        BookReferenceImportArgs::decl(&config),
        BookMetadataView::decl(&config),
        ReferenceChapterView::decl(&config),
        ReferenceMapping::decl(&config),
        BookReferenceView::decl(&config),
        BookReferenceMapArgs::decl(&config),
        BookReplacePreviewArgs::decl(&config),
        BookReplaceApplyArgs::decl(&config),
        BookReplaceChange::decl(&config),
        BookReplacePreview::decl(&config),
        BookExportArgs::decl(&config),
        ListMangaPagesArgs::decl(&config),
        GetMangaPageArgs::decl(&config),
        UpdateMangaRegionArgs::decl(&config),
        UpdateMangaMaskArgs::decl(&config),
        UpdateMangaOrderArgs::decl(&config),
        MangaStageOptions::decl(&config),
        StartMangaStageArgs::decl(&config),
        ReviewMangaPageArgs::decl(&config),
        MangaExportArgs::decl(&config),
        JobArgs::decl(&config),
        ListJobsArgs::decl(&config),
        BookExportFormat::decl(&config),
        MangaExportFormat::decl(&config),
        IncompletePolicy::decl(&config),
        ReviewDecision::decl(&config),
        RegionPatch::decl(&config),
    ];
    declarations
        .into_iter()
        .map(|declaration| format!("export {declaration}\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_reject_noncanonical_and_overflow_values() {
        for value in ["-1", "+1", "01", " 1", "1.0", "9223372036854775808"] {
            assert!(Revision(value.into()).value().is_err(), "{value}");
        }
        assert_eq!(
            Revision("9007199254740993".into()).value().unwrap(),
            9007199254740993
        );
    }

    #[test]
    fn selection_is_resolved_in_source_order_and_rejects_invalid_ids() {
        let ids = vec!["c".into(), "a".into(), "b".into()];
        let selected = EntitySelection::ExplicitIds {
            ids: vec!["b".into(), "c".into()],
        };
        assert_eq!(selected.resolve(&ids).unwrap(), vec!["c", "b"]);
        for requested in [vec!["a", "a"], vec!["missing"], vec![]] {
            assert!(EntitySelection::ExplicitIds {
                ids: requested.into_iter().map(String::from).collect()
            }
            .resolve(&ids)
            .is_err());
        }
        assert_eq!(
            EntitySelection::Range {
                first: "c".into(),
                last: "a".into()
            }
            .resolve(&ids)
            .unwrap(),
            vec!["c", "a"]
        );
        assert!(EntitySelection::Range {
            first: "b".into(),
            last: "c".into()
        }
        .resolve(&ids)
        .is_err());
    }

    #[test]
    fn edits_require_expected_revision_and_reject_legacy_indices() {
        let mut request = serde_json::json!({"projectId": ProjectId::new(), "blockId": "block-1", "text": "Translation", "expectedRevision": "0"});
        assert!(serde_json::from_value::<UpdateBookBlockArgs>(request.clone()).is_ok());
        request["chapterIndex"] = 0.into();
        assert!(serde_json::from_value::<UpdateBookBlockArgs>(request.clone()).is_err());
        request.as_object_mut().unwrap().remove("chapterIndex");
        request.as_object_mut().unwrap().remove("expectedRevision");
        assert!(serde_json::from_value::<UpdateBookBlockArgs>(request).is_err());
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookPresentation {
    pub title: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub instructions: String,
    pub cover_asset_id: Option<String>,
    pub revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateBookPresentationArgs {
    pub project_id: ProjectId,
    pub title: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub instructions: String,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetBookCoverArgs {
    pub project_id: ProjectId,
    pub path: Option<String>,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase")]
pub struct AssistantMessage {pub id:String,pub role:String,pub text:String}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase")]
pub struct AssistantProposal {pub id:String,pub kind:String,pub before:String,pub after:String}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase")]
pub struct AssistantView {pub messages:Vec<AssistantMessage>,pub proposals:Vec<AssistantProposal>}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ProviderEntry {
    pub id:String, pub name:String, pub base_url:String, pub model:String,
    pub temperature:f32, pub max_output_tokens:u32, pub timeout_seconds:u32,
    pub network_retries:u32, pub has_key:bool, pub revision:Revision,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SaveProviderArgs {pub profile:ProviderEntry,pub credential:Option<String>,pub expected_revision:Option<Revision>}

#[derive(Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all="snake_case")]
pub enum BookSearchSide {Source,Translation}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct BookSearchArgs {pub project_id:ProjectId,pub query:String,pub side:BookSearchSide,pub case_sensitive:bool,pub cursor:Option<String>,pub limit:u32}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase")]
pub struct BookSearchMatch {pub chapter_id:ChapterId,pub block_id:BlockId,pub title:String,pub snippet:String}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase")]
pub struct BookSearchPage {pub matches:Vec<BookSearchMatch>,pub next_cursor:Option<String>}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct StartBookRetargetArgs {
    pub project_id: ProjectId,
    pub term_id: String,
    pub expected_revision: Revision,
    pub old_target: String,
    pub max_chapters: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase")]
pub struct BookRetargetPreview { pub chapters: u32, pub fragments: u32 }
