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
//! 1. **Before translation:** [`relevant_terms`] finds glossary terms present in
//!    the chapter text; the orchestrator injects them into the prompt as a
//!    mandatory dictionary (see `translator::prompt::user_prompt`).
//! 2. **After translation:** [`build_extraction_prompt`] + a light model call +
//!    [`parse_extracted_terms`] pull new names/terms out of the translated
//!    chapter, then [`merge`] folds them in. On conflict the already-fixed canon
//!    is kept; the new occurrence only bumps the frequency counter. `pinned`
//!    entries (edited by hand from the UI) are never changed by extraction.
//!
//! This module is pure (no network): the actual model call is done by the
//! orchestrator with `DeepSeekClient`, keeping the glossary trivially testable.
//! Persisted in SQLite by `state`.

use anyhow::{Context, Result};
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

impl TermKind {
    /// Stable lowercase label (used in prompts and DB).
    pub fn label(self) -> &'static str {
        match self {
            TermKind::Person => "person",
            TermKind::Location => "location",
            TermKind::Organization => "organization",
            TermKind::Term => "term",
        }
    }

    /// Parse from a label; unknown values fall back to `Term`.
    pub fn from_label(s: &str) -> TermKind {
        match s.trim().to_ascii_lowercase().as_str() {
            "person" => TermKind::Person,
            "location" => TermKind::Location,
            "organization" => TermKind::Organization,
            _ => TermKind::Term,
        }
    }
}

/// A single glossary entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Term {
    /// Source (Chinese).
    pub source: String,
    /// Canonical translation (Russian).
    pub target: String,
    pub kind: TermKind,
    /// How many times it occurred — for prioritization and conflict resolution.
    pub frequency: u32,
    /// Whether the translation was fixed by hand (not overwritten by extraction).
    pub pinned: bool,
}

/// Find glossary terms that occur in the chapter text (Phase 1).
///
/// Returned most-frequent first (then longest source), so the prompt lists the
/// most important, most specific terms first if it has to be trimmed.
pub fn relevant_terms<'a>(glossary: &'a [Term], chapter_text: &str) -> Vec<&'a Term> {
    let mut hits: Vec<&Term> = glossary
        .iter()
        .filter(|t| !t.source.is_empty() && chapter_text.contains(t.source.as_str()))
        .collect();
    hits.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then_with(|| b.source.chars().count().cmp(&a.source.chars().count()))
            .then_with(|| a.source.cmp(&b.source))
    });
    hits
}

/// Merge newly extracted terms into the glossary (Phase 2).
///
/// Existing entries keep their canonical `target`/`kind` (whether pinned or not)
/// and just accumulate frequency; genuinely new terms are appended.
pub fn merge(glossary: &mut Vec<Term>, new_terms: Vec<Term>) {
    for nt in new_terms {
        match glossary.iter_mut().find(|t| t.source == nt.source) {
            Some(existing) => {
                existing.frequency = existing.frequency.saturating_add(nt.frequency.max(1));
            }
            None => glossary.push(nt),
        }
    }
}

/// Build the (system, user) prompt for extracting terms from a translated
/// chapter. The orchestrator sends this via `DeepSeekClient::translate`, then
/// passes the reply to [`parse_extracted_terms`].
pub fn build_extraction_prompt(source_text: &str, translated_text: &str) -> (String, String) {
    let system = "You extract named entities from a Chinese→Russian novel translation so they \
         stay consistent across chapters. Given a source passage in Chinese and its Russian \
         translation, list the proper nouns and setting-specific terms: character names \
         (person), places (location), organizations/sects (organization), and cultivation or \
         setting terms (term). For each, give the Chinese source form and the exact Russian \
         rendering used in the translation. Ignore ordinary words. Respond with ONLY a JSON \
         array (no prose, no code fences); each item: \
         {\"source\":\"…\",\"target\":\"…\",\"kind\":\"person|location|organization|term\"}."
        .to_string();

    let user = format!(
        "SOURCE (Chinese):\n{source}\n\nTRANSLATION (Russian):\n{translated}\n\nJSON array:",
        source = source_text,
        translated = translated_text,
    );

    (system, user)
}

/// Shape of one extracted entry as returned by the model.
#[derive(Deserialize)]
struct RawTerm {
    source: String,
    target: String,
    #[serde(default)]
    kind: String,
}

/// Parse the model's extraction reply into `Term`s (frequency 1, not pinned).
///
/// Tolerant of surrounding prose or ```code fences``` — it takes the outermost
/// `[ … ]` as the JSON array.
pub fn parse_extracted_terms(raw: &str) -> Result<Vec<Term>> {
    let json = slice_json_array(raw)?;
    let raws: Vec<RawTerm> =
        serde_json::from_str(json).context("parsing extracted-terms JSON")?;
    Ok(raws
        .into_iter()
        .filter(|r| !r.source.trim().is_empty() && !r.target.trim().is_empty())
        .map(|r| Term {
            source: r.source.trim().to_string(),
            target: r.target.trim().to_string(),
            kind: TermKind::from_label(&r.kind),
            frequency: 1,
            pinned: false,
        })
        .collect())
}

/// Extract the outermost JSON array `[ … ]` from a possibly-decorated reply.
fn slice_json_array(raw: &str) -> Result<&str> {
    let start = raw.find('[').context("extraction reply has no JSON array")?;
    let end = raw.rfind(']').context("extraction reply has no closing ']'")?;
    if end <= start {
        anyhow::bail!("extraction reply has malformed JSON array");
    }
    Ok(&raw[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(source: &str, target: &str, kind: TermKind, freq: u32, pinned: bool) -> Term {
        Term {
            source: source.into(),
            target: target.into(),
            kind,
            frequency: freq,
            pinned,
        }
    }

    #[test]
    fn relevant_terms_finds_present_and_orders_by_frequency() {
        let glossary = vec![
            term("王林", "Ван Линь", TermKind::Person, 9, true),
            term("南凰洲", "Наньхуанчжоу", TermKind::Location, 3, false),
            term("秃鹫", "гриф", TermKind::Term, 1, false),
        ];
        let hits = relevant_terms(&glossary, "王林走进南凰洲。");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].source, "王林"); // higher frequency first
        assert_eq!(hits[1].source, "南凰洲");
    }

    #[test]
    fn merge_keeps_canon_and_bumps_frequency() {
        let mut glossary = vec![term("王林", "Ван Линь", TermKind::Person, 2, true)];
        // A later chapter's extraction proposes a different rendering — must be ignored.
        merge(
            &mut glossary,
            vec![term("王林", "Ванлинь", TermKind::Person, 1, false)],
        );
        assert_eq!(glossary.len(), 1);
        assert_eq!(glossary[0].target, "Ван Линь"); // canon wins
        assert!(glossary[0].pinned); // pin preserved
        assert_eq!(glossary[0].frequency, 3); // 2 + 1
    }

    #[test]
    fn merge_appends_new_terms() {
        let mut glossary = vec![term("王林", "Ван Линь", TermKind::Person, 1, false)];
        merge(
            &mut glossary,
            vec![term("南凰洲", "Наньхуанчжоу", TermKind::Location, 1, false)],
        );
        assert_eq!(glossary.len(), 2);
    }

    #[test]
    fn parse_extracted_terms_tolerates_code_fences() {
        let raw = "Here you go:\n```json\n[\
            {\"source\":\"王林\",\"target\":\"Ван Линь\",\"kind\":\"person\"},\
            {\"source\":\"南凰洲\",\"target\":\"Наньхуанчжоу\",\"kind\":\"location\"}\
            ]\n```";
        let terms = parse_extracted_terms(raw).unwrap();
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].kind, TermKind::Person);
        assert_eq!(terms[1].kind, TermKind::Location);
        assert_eq!(terms[0].frequency, 1);
        assert!(!terms[0].pinned);
    }

    #[test]
    fn parse_extracted_terms_skips_empty_and_defaults_kind() {
        let raw = "[{\"source\":\"秃鹫\",\"target\":\"гриф\"},{\"source\":\"\",\"target\":\"x\"}]";
        let terms = parse_extracted_terms(raw).unwrap();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].kind, TermKind::Term); // missing kind -> Term
    }
}
