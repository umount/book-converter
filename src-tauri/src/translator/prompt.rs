//! Building translation prompts.
//!
//! A chapter's prompt is assembled from: base instructions (system) + a mandatory
//! glossary dictionary + an optional style exemplar (from a reference translation)
//! + the rolling context (running summary + previous chapter tail) + the text.
//! The glossary keeps terms consistent; the rolling context keeps the narrative
//! consistent across a sequential run.

use std::fmt::Write as _;

use crate::config::Config;
use crate::glossary::{Term, TermKind};

/// Everything except the chapter text that shapes a translation request.
#[derive(Default)]
pub struct PromptContext<'a> {
    /// Glossary terms present in this chapter (mandatory renderings).
    pub terms: &'a [&'a Term],
    /// Running summary of the story so far.
    pub summary: Option<&'a str>,
    /// Closing lines of the previous chapter's translation (immediate continuity).
    pub prev_tail: Option<&'a str>,
    /// A professional excerpt to match in tone/register.
    pub style: Option<&'a str>,
}

/// System prompt: role and general translation rules.
pub fn system_prompt(config: &Config) -> String {
    format!(
        "You are a professional literary translator from {src} to {tgt}. \
         Translate in a natural, coherent, literary style, preserving voice and \
         paragraph structure. Keep the chapter title. Do not add explanations, \
         notes, or the original text — output the translation only.",
        src = config.source_lang,
        tgt = config.target_lang,
    )
}

/// User prompt: dictionary + style + rolling context + the text to translate.
pub fn user_prompt(ctx: &PromptContext, text: &str) -> String {
    let mut out = String::new();

    if !ctx.terms.is_empty() {
        out.push_str(
            "Use exactly these fixed translations for the following terms \
             (source → target). Do not translate them any other way:\n",
        );
        for t in ctx.terms {
            let _ = writeln!(out, "- {} → {} [{}]", t.source, t.target, kind_label(t.kind));
        }
        out.push('\n');
    }

    if let Some(style) = ctx.style {
        if !style.trim().is_empty() {
            out.push_str(
                "Match the tone and register of this excerpt from an existing \
                 professional translation (do not copy it, it is only a style guide):\n",
            );
            out.push_str(style.trim());
            out.push_str("\n\n");
        }
    }

    if let Some(summary) = ctx.summary {
        if !summary.trim().is_empty() {
            out.push_str("Story so far (context only, do not translate):\n");
            out.push_str(summary.trim());
            out.push_str("\n\n");
        }
    }

    if let Some(tail) = ctx.prev_tail {
        if !tail.trim().is_empty() {
            out.push_str("The previous chapter ended with (for continuity, do not translate):\n");
            out.push_str(tail.trim());
            out.push_str("\n\n");
        }
    }

    out.push_str("Translate the following text:\n\n");
    out.push_str(text);
    out
}

/// Build a (system, user) prompt to fold a freshly translated chapter into the
/// running summary. Keeps continuity compact so the prompt stays small.
pub fn build_summary_prompt(
    config: &Config,
    prev_summary: &str,
    chapter_translation: &str,
) -> (String, String) {
    let system = format!(
        "You maintain a concise running synopsis of a novel in {tgt}. Given the \
         summary so far and the next chapter, return an updated synopsis of at most \
         200 words that preserves key plot points, characters, relationships, and \
         unresolved threads. Output only the updated synopsis.",
        tgt = config.target_lang,
    );
    let user = format!(
        "Summary so far:\n{prev}\n\nNext chapter:\n{chapter}\n\nUpdated synopsis:",
        prev = if prev_summary.trim().is_empty() { "(none yet)" } else { prev_summary.trim() },
        chapter = chapter_translation,
    );
    (system, user)
}

/// Human-readable category label used in the prompt dictionary.
fn kind_label(kind: TermKind) -> &'static str {
    match kind {
        TermKind::Person => "person",
        TermKind::Location => "location",
        TermKind::Organization => "organization",
        TermKind::Term => "term",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(source: &str, target: &str, kind: TermKind) -> Term {
        Term { source: source.into(), target: target.into(), kind, frequency: 1, pinned: false }
    }

    #[test]
    fn includes_glossary_and_text() {
        let wang = term("王林", "Ван Линь", TermKind::Person);
        let ctx = PromptContext { terms: &[&wang], ..Default::default() };
        let p = user_prompt(&ctx, "王林走了。");
        assert!(p.contains("王林 → Ван Линь [person]"));
        assert!(p.trim_end().ends_with("王林走了。"));
    }

    #[test]
    fn no_dictionary_header_when_empty() {
        let ctx = PromptContext::default();
        let p = user_prompt(&ctx, "text");
        assert!(!p.contains("fixed translations"));
        assert!(p.contains("Translate the following text:"));
    }

    #[test]
    fn includes_context_sections() {
        let ctx = PromptContext {
            summary: Some("Wang Lin survived."),
            prev_tail: Some("…he closed his eyes."),
            style: Some("A professional excerpt."),
            ..Default::default()
        };
        let p = user_prompt(&ctx, "text");
        assert!(p.contains("Story so far"));
        assert!(p.contains("Wang Lin survived."));
        assert!(p.contains("previous chapter ended with"));
        assert!(p.contains("style guide"));
    }

    #[test]
    fn summary_prompt_handles_empty_previous() {
        let cfg = Config::default();
        let (_s, u) = build_summary_prompt(&cfg, "", "Chapter text.");
        assert!(u.contains("(none yet)"));
        assert!(u.contains("Chapter text."));
    }
}
