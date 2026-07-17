//! Term glossary — ensures consistent translation of names, locations, and
//! terminology across the whole book.
//!
//! ## Why
//! An LLM translates each chapter independently and, without a shared
//! dictionary, tends to translate the protagonist's name inconsistently
//! (e.g. the same 王林 coming out as three different spellings) and mix up sect
//! names, locations, and cultivation terms. The glossary fixes the canonical
//! translation once and enforces it in all subsequent chapters.
//!
//! ## How it works (two-phase cycle per chapter)
//! 1. **Before translation:** find glossary terms that occur in the chapter
//!    text and inject them into the prompt as a mandatory dictionary
//!    (see [`relevant_terms`] → `translator::prompt`).
//! 2. **After translation:** with a separate light request, extract new
//!    names/terms from the chapter ([`extract_terms`]) and merge them into the
//!    glossary ([`merge`]). On conflict the already-fixed canon is kept; the new
//!    term only bumps its frequency counter.
//!
//! Stored in the same SQLite database as progress (`state`).

use serde::{Deserialize, Serialize};

/// Term category — affects strictness and the model hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TermKind {
    /// Character name.
    Person,
    /// Location (city, mountain, realm…).
    Location,
    /// Organization (sect, clan, order…).
    Organization,
    /// Setting term (cultivation level, technique, artifact…).
    Term,
}

/// A single glossary entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Term {
    /// Source (Chinese).
    pub source: String,
    /// Canonical translation (Russian).
    pub target: String,
    pub kind: TermKind,
    /// How many times it occurred — for prioritization and conflict resolution.
    pub frequency: u32,
    /// Whether the translation was fixed by hand (not overwritten by auto-extraction).
    pub pinned: bool,
}

/// Find glossary terms that occur in the chapter text.
/// The result goes into the translation prompt as a mandatory dictionary.
pub fn relevant_terms<'a>(_glossary: &'a [Term], _chapter_text: &str) -> Vec<&'a Term> {
    todo!("find term substrings in the chapter text")
}

/// Extract new terms from the translated chapter (a light request to the model
/// returning "source → translation" pairs with a category).
pub async fn extract_terms(_source_text: &str, _translated_text: &str) -> anyhow::Result<Vec<Term>> {
    todo!("extract new names/terms with a second DeepSeek request")
}

/// Merge new terms into the glossary: existing ones bump `frequency`,
/// pinned/canon entries are left untouched; new ones are added.
pub fn merge(_glossary: &mut Vec<Term>, _new_terms: Vec<Term>) {
    todo!("merge with conflict resolution in favor of the canon")
}
