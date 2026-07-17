//! Building translation prompts.
//!
//! The prompt is assembled dynamically: base instructions (system) + relevant
//! glossary terms as a mandatory dictionary + an optional previous-chapter
//! summary + the text (user). Injecting the glossary is what keeps names and
//! terms consistent across chapters.

use std::fmt::Write as _;

use crate::config::Config;
use crate::glossary::{Term, TermKind};

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

/// User prompt: mandatory term dictionary + optional summary + the text.
pub fn user_prompt(terms: &[&Term], summary: Option<&str>, text: &str) -> String {
    let mut out = String::new();

    if !terms.is_empty() {
        out.push_str(
            "Use exactly these fixed translations for the following terms \
             (source → target). Do not translate them any other way:\n",
        );
        for t in terms {
            // `writeln!` into a String never fails.
            let _ = writeln!(
                out,
                "- {} → {} [{}]",
                t.source,
                t.target,
                kind_label(t.kind)
            );
        }
        out.push('\n');
    }

    if let Some(summary) = summary {
        if !summary.trim().is_empty() {
            out.push_str("Context from the previous chapter (do not translate this, for continuity only):\n");
            out.push_str(summary.trim());
            out.push_str("\n\n");
        }
    }

    out.push_str("Translate the following text:\n\n");
    out.push_str(text);
    out
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
        Term {
            source: source.into(),
            target: target.into(),
            kind,
            frequency: 1,
            pinned: false,
        }
    }

    #[test]
    fn includes_glossary_and_text() {
        let wang = term("王林", "Ван Линь", TermKind::Person);
        let terms = vec![&wang];
        let p = user_prompt(&terms, None, "王林走了。");
        assert!(p.contains("王林 → Ван Линь [person]"));
        assert!(p.contains("Translate the following text:"));
        assert!(p.trim_end().ends_with("王林走了。"));
    }

    #[test]
    fn no_dictionary_header_when_empty() {
        let p = user_prompt(&[], None, "text");
        assert!(!p.contains("fixed translations"));
        assert!(p.contains("Translate the following text:"));
    }

    #[test]
    fn includes_summary_when_present() {
        let p = user_prompt(&[], Some("Previously..."), "text");
        assert!(p.contains("Context from the previous chapter"));
        assert!(p.contains("Previously..."));
    }
}
