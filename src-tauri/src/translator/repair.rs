//! Repairing the lines of a translation that kept words in the wrong language.
//!
//! Detection ([`crate::textutil::foreign_fragments`]) is local and cheap: it
//! walks the translated text and reports runs of unexpected script. The repair
//! then has to get those words out, and what it sends the model is the whole
//! point of this module.
//!
//! **It sends the affected lines, not the chapter.** Re-translating a whole
//! chapter to fix three words costs a chapter of tokens per pass, up to two
//! passes, on a book where the pass fires often. Worse, it puts every correct
//! paragraph back at risk: the old code carried a guard that threw away a repair
//! which came back much shorter than the original, because a model asked to
//! rewrite 4000 characters "changing nothing else" sometimes summarises instead.
//! With line-scoped repair a bad line can only damage itself, the guard becomes
//! a per-line check, and a chapter with two bad paragraphs costs two paragraphs.
//!
//! The reply is JSON (`{"lines": [{"n": 12, "text": "…"}]}`) so lines are put
//! back by the number they carry rather than by counting lines in a prose reply,
//! which desynchronises the moment the model merges or splits one. Batches are
//! capped so that reply can never hit the output token limit, which JSON cannot
//! recover from.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::Config;

/// Largest amount of text put into one repair request. Several small requests
/// beat one big one here: a JSON reply that hits the output limit is lost
/// entirely, and a smaller batch also keeps the model's attention on the words
/// it was asked to fix.
pub const MAX_BATCH_CHARS: usize = 6000;

/// One line handed to the model, identified by its position in the chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberedLine {
    pub n: usize,
    pub text: String,
}

/// The model's repair reply.
#[derive(Deserialize)]
struct RawReply {
    #[serde(default)]
    lines: Vec<RawLine>,
}

#[derive(Deserialize)]
struct RawLine {
    n: usize,
    #[serde(default)]
    text: String,
}

/// Positions of the lines that contain at least one of `fragments`.
///
/// Case-sensitive on purpose: `fragments` come from the text itself, in the form
/// they appear there.
pub fn lines_with_fragments(lines: &[String], fragments: &[String]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| fragments.iter().any(|f| line.contains(f.as_str())))
        .map(|(n, _)| n)
        .collect()
}

/// Split the lines to repair into batches no larger than `max_chars`.
///
/// A single line longer than the cap still goes out alone rather than being
/// split: half a paragraph gives the model no context to repair it with.
pub fn batches(lines: &[NumberedLine], max_chars: usize) -> Vec<Vec<NumberedLine>> {
    let mut out: Vec<Vec<NumberedLine>> = Vec::new();
    let mut current: Vec<NumberedLine> = Vec::new();
    let mut size = 0usize;
    for line in lines {
        let len = line.text.chars().count();
        if !current.is_empty() && size + len > max_chars {
            out.push(std::mem::take(&mut current));
            size = 0;
        }
        size += len;
        current.push(line.clone());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Build the `(system, user)` prompt repairing one batch of lines.
pub fn build_prompt(
    config: &Config,
    fragments: &[String],
    batch: &[NumberedLine],
) -> (String, String) {
    let tgt = &config.target_lang;
    let no_roman = if target_is_english(config) {
        ""
    } else {
        " or in an English romanization"
    };
    let system = format!(
        "You clean up a {tgt} literary translation. Each input line still contains \
         words that are not in {tgt}. Replace every one of them with proper {tgt}: \
         personal names are transliterated by sound (never translated by meaning); \
         place, sect and technique names are translated by meaning when they carry \
         one, transliterated otherwise; everything else is translated. Never leave \
         a word in the original script{no_roman}, and never keep a foreign stem \
         with {tgt} endings attached, replace the whole word. Replace every \
         occurrence, including capitalised, plural and inflected forms, and any \
         phrase built around it.\n\
         Change nothing else. Keep each line's wording, length and punctuation \
         otherwise exactly as they are, and never merge, split, drop or reorder \
         lines.\n\
         Reply with one json object: {{\"lines\":[{{\"n\":<the line number you were \
         given>,\"text\":\"<the corrected line>\"}}]}}. Return every line you were \
         given, each with the same n it came in with, and nothing else."
    );

    let mut user = String::from("Words that must not remain:\n");
    for f in fragments {
        let _ = writeln!(user, "- {f}");
    }
    user.push_str("\nLines to correct:\n");
    for line in batch {
        let _ = writeln!(user, "[{}] {}", line.n, line.text);
    }
    user.push_str("\njson:");
    (system, user)
}

/// Parse the model's repair reply into numbered lines.
pub fn parse_reply(raw: &str) -> Result<Vec<NumberedLine>> {
    let json = slice_json_object(raw).context("repair reply has no JSON object")?;
    let parsed: RawReply = serde_json::from_str(json).context("parsing repair JSON")?;
    Ok(parsed
        .lines
        .into_iter()
        .map(|l| NumberedLine {
            n: l.n,
            text: l.text,
        })
        .collect())
}

/// Apply repaired lines back onto `lines`, returning how many actually changed.
///
/// A repair is rejected, keeping the original line, when it is out of range,
/// empty where the original was not, or less than half the original's length.
/// Those are the shapes a derailed model returns (a summary, a refusal, an
/// apology), and on a single line they are cheap to detect and cheap to skip.
pub fn splice(lines: &mut [String], fixed: &[NumberedLine]) -> usize {
    let mut changed = 0;
    for line in fixed {
        let Some(slot) = lines.get_mut(line.n) else {
            tracing::warn!(n = line.n, "repair returned a line number out of range");
            continue;
        };
        let new = line.text.trim();
        let original_len = slot.trim().chars().count();
        if original_len == 0 {
            continue;
        }
        if new.is_empty() {
            tracing::warn!(n = line.n, "repair emptied a line; keeping the original");
            continue;
        }
        if new.chars().count() * 2 < original_len {
            tracing::warn!(
                n = line.n,
                "repair returned a much shorter line; keeping the original"
            );
            continue;
        }
        if new != slot.trim() {
            *slot = new.to_string();
            changed += 1;
        }
    }
    changed
}

/// True when the target language is English, where a rule against English
/// romanizations would be nonsense.
fn target_is_english(config: &Config) -> bool {
    config.target_lang.trim().eq_ignore_ascii_case("english")
}

fn slice_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| &raw[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_only_the_lines_that_need_work() {
        let text = lines(&[
            "Глава 5",
            "Ван Линь шагнул вперёд.",
            "Он посмотрел на 王林 и кивнул.",
            "Ветер стих.",
            "Это была cultivation базы.",
        ]);
        let bad = vec!["王林".to_string(), "cultivation".to_string()];
        assert_eq!(lines_with_fragments(&text, &bad), vec![2, 4]);
    }

    #[test]
    fn a_clean_translation_needs_no_request() {
        let text = lines(&["Глава 5", "Ветер стих."]);
        assert!(lines_with_fragments(&text, &["王林".to_string()]).is_empty());
    }

    #[test]
    fn batches_respect_the_cap_but_never_split_a_line() {
        let long = NumberedLine {
            n: 0,
            text: "я".repeat(50),
        };
        let a = NumberedLine {
            n: 1,
            text: "я".repeat(30),
        };
        let b = NumberedLine {
            n: 2,
            text: "я".repeat(30),
        };
        let out = batches(&[long.clone(), a.clone(), b.clone()], 40);
        assert_eq!(out.len(), 3, "{out:?}");
        assert_eq!(out[0], vec![long]);
        assert_eq!(out[1], vec![a]);
        assert_eq!(out[2], vec![b]);
    }

    #[test]
    fn batches_pack_lines_up_to_the_cap() {
        let l = |n: usize| NumberedLine {
            n,
            text: "я".repeat(10),
        };
        let out = batches(&[l(0), l(1), l(2), l(3)], 25);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 2);
        assert_eq!(out[1].len(), 2);
    }

    #[test]
    fn prompt_carries_the_numbers_and_the_fragments() {
        let (system, user) = build_prompt(
            &Config::default(),
            &["王林".into()],
            &[NumberedLine {
                n: 7,
                text: "Он увидел 王林.".into(),
            }],
        );
        assert!(user.contains("- 王林"));
        assert!(user.contains("[7] Он увидел 王林."));
        assert!(system.to_lowercase().contains("json"));
        assert!(system.contains("never merge, split, drop or reorder"));
    }

    #[test]
    fn prompt_covers_inflected_forms() {
        let (system, _u) = build_prompt(
            &Config::default(),
            &["cultivation".into()],
            &[NumberedLine {
                n: 0,
                text: "т".into(),
            }],
        );
        assert!(system.contains("inflected forms"));
    }

    /// A rule against English romanizations is nonsense when English is the target.
    #[test]
    fn english_target_drops_the_romanization_rule() {
        let cfg = Config {
            target_lang: "English".into(),
            ..Config::default()
        };
        let (system, _u) = build_prompt(
            &cfg,
            &["王林".into()],
            &[NumberedLine {
                n: 0,
                text: "text".into(),
            }],
        );
        assert!(!system.contains("English romanization"));
    }

    #[test]
    fn parses_the_reply() {
        let raw = r#"{"lines":[{"n":2,"text":"Он посмотрел на Ван Линя."}]}"#;
        let out = parse_reply(raw).unwrap();
        assert_eq!(
            out,
            vec![NumberedLine {
                n: 2,
                text: "Он посмотрел на Ван Линя.".into()
            }]
        );
    }

    #[test]
    fn splices_by_number_not_by_order() {
        let mut text = lines(&["Глава 5", "первая", "вторая", "третья"]);
        let fixed = vec![
            NumberedLine {
                n: 3,
                text: "третья исправленная".into(),
            },
            NumberedLine {
                n: 1,
                text: "первая исправленная".into(),
            },
        ];
        assert_eq!(splice(&mut text, &fixed), 2);
        assert_eq!(text[1], "первая исправленная");
        assert_eq!(text[2], "вторая");
        assert_eq!(text[3], "третья исправленная");
    }

    /// A derailed model can no longer damage the chapter, only lose its own line.
    #[test]
    fn rejects_a_gutted_line() {
        let mut text = lines(&["Довольно длинная строка перевода на месте."]);
        let before = text[0].clone();
        assert_eq!(
            splice(
                &mut text,
                &[NumberedLine {
                    n: 0,
                    text: "ок".into()
                }]
            ),
            0
        );
        assert_eq!(text[0], before);
        assert_eq!(
            splice(
                &mut text,
                &[NumberedLine {
                    n: 0,
                    text: "  ".into()
                }]
            ),
            0
        );
        assert_eq!(text[0], before);
    }

    #[test]
    fn ignores_a_line_number_out_of_range() {
        let mut text = lines(&["строка"]);
        assert_eq!(
            splice(
                &mut text,
                &[NumberedLine {
                    n: 9,
                    text: "что-то".into()
                }]
            ),
            0
        );
        assert_eq!(text[0], "строка");
    }
}
