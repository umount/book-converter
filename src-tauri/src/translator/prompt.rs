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
    /// Optional user instruction for this chapter only (not the glossary).
    pub user_note: Option<&'a str>,
    /// The book this chapter belongs to (original title, author, known
    /// translated title). Models often know the work, and naming it helps them
    /// place names and setting terminology.
    pub book: Option<BookRef<'a>>,
}

/// Identity of the book being translated, as far as it is known.
#[derive(Default, Clone, Copy)]
pub struct BookRef<'a> {
    pub title: Option<&'a str>,
    pub author: Option<&'a str>,
    /// Title in the target language, when one is already known.
    pub title_translated: Option<&'a str>,
}

impl BookRef<'_> {
    fn is_empty(&self) -> bool {
        [self.title, self.author, self.title_translated]
            .iter()
            .all(|v| v.map(|s| s.trim().is_empty()).unwrap_or(true))
    }
}

/// System prompt: role and general translation rules.
///
/// The language rules are deliberately blunt: models drift into leaving source
/// script in place for names and terms it is unsure about, or into borrowing an
/// English romanization of them. Both are checked for after translation
/// (`textutil::foreign_fragments`).
pub fn system_prompt(config: &Config) -> String {
    format!(
        "You are a professional literary translator from {src} to {tgt}. \
         Translate in a natural, coherent, literary style, preserving voice and \
         paragraph structure. Keep the chapter title. Do not add explanations, \
         notes, or the original text — output the translation only.\n\
         \n\
         Language rules (strict):\n\
         - Write the entire output in {tgt}. Not a single word, name or \
           interjection may remain in {src} or appear in any third language.\n\
         - Personal names are transliterated by sound into {tgt}, using the \
           transcription conventional for the {src}-{tgt} pair, and are never \
           translated by meaning: \"Harry Potter\" becomes the {tgt} spelling of \
           \"Harry Potter\", not a rendering of what \"potter\" means. The same holds \
           for {src} names: transliterate how the name sounds, do not translate \
           what its characters mean.\n\
         - Place names, sects, techniques and artefacts usually carry meaning in \
           this genre, and that meaning is part of the story: translate it \
           (\"Blood Lake\", \"Valley of Sorrow\", \"Heavenly Sword Sect\"). \
           Transliterate such a name only when it has no transparent meaning, or \
           when it is a real-world place with an established {tgt} name.\n\
         - Whichever way a name is rendered, render it that way everywhere.\n\
         - Never leave a name in the original script, and never fall back on an \
           English romanization of it.\n\
         - A term you are unsure about is still translated or transliterated — \
           leaving the original word in is never an acceptable fallback.\n\
         - Keep numbers, and punctuation appropriate for {tgt}.",
        src = config.source_lang,
        tgt = config.target_lang,
    )
}

/// User prompt: dictionary + style + rolling context + the text to translate.
pub fn user_prompt(ctx: &PromptContext, text: &str) -> String {
    let mut out = String::new();

    if let Some(book) = ctx.book.filter(|b| !b.is_empty()) {
        out.push_str("This chapter is from the following work");
        if let Some(t) = book.title.filter(|s| !s.trim().is_empty()) {
            let _ = write!(out, ", titled \"{}\"", t.trim());
        }
        if let Some(a) = book.author.filter(|s| !s.trim().is_empty()) {
            let _ = write!(out, ", by {}", a.trim());
        }
        if let Some(tt) = book.title_translated.filter(|s| !s.trim().is_empty()) {
            let _ = write!(out, " (published in translation as \"{}\")", tt.trim());
        }
        out.push_str(
            ". If you know this work, use that knowledge for names, lore and setting \
             terminology. The chapter text and the dictionary below always take \
             precedence over your recollection; never import plot details that are \
             not in the text.\n\n",
        );
    }

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

    if let Some(note) = ctx.user_note {
        if !note.trim().is_empty() {
            out.push_str(
                "Additional instructions from the user for THIS chapter only \
                 (follow them; they override conflicting defaults for this chapter):\n",
            );
            out.push_str(note.trim());
            out.push_str("\n\n");
        }
    }

    out.push_str("Translate the following text:\n\n");
    out.push_str(text);
    out
}

/// Build a (system, user) prompt asking the model to repair a translation that
/// still carries words in the wrong language: the fragments are named explicitly,
/// and everything else must come back untouched.
pub fn build_language_fix_prompt(
    config: &Config,
    fragments: &[String],
    text: &str,
) -> (String, String) {
    let system = format!(
        "You clean up a {tgt} literary translation. The text below still contains \
         words that are not in {tgt}. Replace every one of them with proper {tgt}: \
         personal names are transliterated by sound (never translated by meaning); \
         place, sect and technique names are translated by meaning when they carry \
         one, transliterated otherwise; everything else is translated. Never leave \
         a word in the original script or in an English romanization. \
         Change nothing else — keep \
         wording, paragraphs and punctuation exactly as they are. Output only the \
         corrected text.",
        tgt = config.target_lang,
    );
    let mut user = String::from("Fragments that must not remain:\n");
    for f in fragments {
        let _ = writeln!(user, "- {f}");
    }
    let _ = write!(user, "\nText:\n{text}");
    (system, user)
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
    fn includes_user_note() {
        let ctx = PromptContext {
            user_note: Some("Render 她 as господин, not госпожа."),
            ..Default::default()
        };
        let p = user_prompt(&ctx, "text");
        assert!(p.contains("Additional instructions from the user"));
        assert!(p.contains("господин, not госпожа"));
    }

    #[test]
    fn summary_prompt_handles_empty_previous() {
        let cfg = Config::default();
        let (_s, u) = build_summary_prompt(&cfg, "", "Chapter text.");
        assert!(u.contains("(none yet)"));
        assert!(u.contains("Chapter text."));
    }

    #[test]
    fn includes_book_identity() {
        let ctx = PromptContext {
            book: Some(BookRef {
                title: Some("光阴之外"),
                author: Some("耳根"),
                title_translated: Some("За гранью времени"),
            }),
            ..Default::default()
        };
        let p = user_prompt(&ctx, "text");
        assert!(p.contains("光阴之外"));
        assert!(p.contains("耳根"));
        assert!(p.contains("За гранью времени"));
    }

    #[test]
    fn no_book_section_when_unknown() {
        let ctx = PromptContext { book: Some(BookRef::default()), ..Default::default() };
        assert!(!user_prompt(&ctx, "text").contains("This chapter is from"));
    }

    #[test]
    fn system_prompt_states_the_language_rules() {
        let cfg = Config::default();
        let s = system_prompt(&cfg);
        assert!(s.contains("entire output in Russian"));
        assert!(s.contains("transliterated by sound"));
        assert!(s.contains("Valley of Sorrow"));
    }

    #[test]
    fn language_fix_prompt_lists_fragments() {
        let cfg = Config::default();
        let (_s, u) = build_language_fix_prompt(&cfg, &["王林".into(), "cultivation".into()], "Текст");
        assert!(u.contains("- 王林"));
        assert!(u.contains("- cultivation"));
        assert!(u.trim_end().ends_with("Текст"));
    }
}
