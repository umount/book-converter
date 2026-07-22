//! DTOs shared by Tauri commands and background jobs.

use serde::{Deserialize, Serialize};

use crate::glossary::Term;

#[derive(Serialize)]
pub struct BookInfo {
    pub title: String,
    pub author: String,
    pub total_chapters: usize,
    pub format: String,
    pub encoding: String,
    pub needs_delimiter: bool,
    pub missing: usize,
    pub duplicates: usize,
}

#[derive(Serialize)]
pub struct RefInfo {
    pub title: String,
    pub chapters: usize,
    pub max_covered: Option<usize>,
    /// How many still-pending chapters were seeded from this reference.
    pub imported: usize,
}

#[derive(Serialize, Clone)]
pub struct Progress {
    pub project: String,
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub pending: usize,
    pub running: bool,
    /// Chapters finished in the current job (0..job_total).
    #[serde(default)]
    pub job_done: usize,
    /// Chapters planned for the current job (e.g. min(limit, pending)).
    #[serde(default)]
    pub job_total: usize,
    /// Reading-order index of the chapter currently being translated.
    #[serde(default)]
    pub current_idx: Option<usize>,
    /// Book chapter number from the title (`第N章`), when known.
    #[serde(default)]
    pub current_number: Option<usize>,
    #[serde(default)]
    pub current_title: Option<String>,
    /// First still-pending chapter's book number (for the resume hint).
    #[serde(default)]
    pub next_number: Option<usize>,
    /// Highest book chapter number in the source (for UI inputs).
    #[serde(default)]
    pub max_number: Option<usize>,
    /// `start` | `chapter_start` | `chapter_done` | `status`
    #[serde(default)]
    pub phase: String,
    /// Duration of the last finished chapter, milliseconds.
    #[serde(default)]
    pub last_ms: Option<u64>,
    /// Estimated remaining seconds for this job (from avg chapter time).
    #[serde(default)]
    pub eta_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TermDto {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub frequency: u32,
    pub pinned: bool,
}

/// A chapter row for the reader's chapter list.
#[derive(Serialize)]
pub struct ChapterRow {
    pub idx: usize,
    pub number: Option<usize>,
    pub title: String,
    /// Translated title when the chapter is done (shown in the chapter picker).
    pub translated_title: Option<String>,
    pub status: String,
    /// Where the translation came from: `"reference"`, `"model"`, or none.
    pub origin: Option<String>,
}

/// Full chapter view: original + translation.
#[derive(Serialize)]
pub struct ChapterView {
    pub idx: usize,
    /// Book chapter number from the title (`第N章`), when known.
    pub number: Option<usize>,
    pub source_title: String,
    pub source: String,
    pub translated_title: Option<String>,
    pub translated: Option<String>,
    pub status: String,
    pub origin: Option<String>,
    /// Optional user instruction for this chapter only (injected into the prompt).
    pub user_prompt: Option<String>,
    /// Rolling story synopsis fed into this chapter's translation prompt.
    pub rolling_summary: Option<String>,
    /// Closing lines of the previous chapter (continuity), for this chapter's prompt.
    pub prev_tail: Option<String>,
}

/// Book cover + metadata for the UI.
#[derive(Serialize)]
pub struct BookDetails {
    pub title: String,
    pub author: String,
    pub title_translated: Option<String>,
    pub author_translated: Option<String>,
    pub summary: Option<String>,
    /// Cover as a `data:` URL, if any.
    pub cover: Option<String>,
}

/// One rename to propagate into the existing translation.
#[derive(Deserialize)]
pub struct RenameChange {
    pub old_target: String,
    pub new_target: String,
    pub kind: String,
}

/// Result of importing a `.bcproj` archive: enough for the frontend to register a
/// project row and open it (from its DB).
#[derive(Serialize)]
pub struct ImportedProject {
    pub name: String,
    pub source_path: String,
}

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub(crate) fn term_to_dto(t: Term) -> TermDto {
    TermDto {
        source: t.source,
        target: t.target,
        kind: t.kind.label().to_string(),
        frequency: t.frequency,
        pinned: t.pinned,
    }
}
