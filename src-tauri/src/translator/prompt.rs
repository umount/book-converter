//! Building translation prompts.
//!
//! The prompt is assembled dynamically: base instructions + relevant glossary
//! terms (mandatory dictionary) + an optional previous-chapter summary.
//! Injecting the glossary is what keeps names/terms consistent.

use crate::config::Config;
use crate::glossary::Term;

/// System prompt: role and general translation rules.
pub fn system_prompt(config: &Config) -> String {
    format!(
        "You are a professional literary translator from {src} to {tgt}. \
         Translate in a natural, coherent, literary style, preserving voice and \
         paragraph structure. Do not add explanations or notes — output the \
         translation only.",
        src = config.source_lang,
        tgt = config.target_lang,
    )
}

/// User prompt: mandatory term dictionary + the text to translate.
///
/// TODO: render `terms` as a list of "source → target (category)" with an
/// instruction to use exactly these renderings, then append `text`.
pub fn user_prompt(_terms: &[&Term], _summary: Option<&str>, _text: &str) -> String {
    todo!("assemble the user prompt: term dictionary + summary + chapter text")
}
