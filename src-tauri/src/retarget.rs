//! Propagate a glossary rename into the already-translated text.
//!
//! A plain find/replace breaks inflected languages: the old rendering appears in
//! many grammatical forms ("Сюй Цина", "Сюй Цину") and changing a name's gender
//! must ripple onto agreeing words ("сказала" → "сказал"). So we do it with the
//! model, but only on the paragraphs that actually mention the old rendering —
//! cheap, and it keeps the rest of the text untouched.
//!
//! This module holds the pure, testable pieces: which paragraphs are candidates,
//! and the rewrite prompt. The job orchestration (threading, events) lives in
//! `commands`.

/// A stem of a single-word `target`: drop a trailing soft/vowel character that
/// inflection typically replaces ("Аня" → "ан", so it matches "Ани"/"Аню").
fn stem(word: &str) -> String {
    let mut chars: Vec<char> = word.trim().to_lowercase().chars().collect();
    if let Some(&last) = chars.last() {
        const TRIM: &[char] =
            &['а', 'я', 'о', 'е', 'ы', 'и', 'й', 'ь', 'ю', 'э', 'a', 'e', 'o', 'y'];
        if TRIM.contains(&last) && chars.len() > 2 {
            chars.pop();
        }
    }
    chars.into_iter().collect()
}

/// Does this paragraph plausibly mention `old_target` (in any inflected form)?
/// Used only to pick candidates — the model makes the final call, so a loose
/// over-match here is harmless (an unaffected paragraph comes back unchanged).
///
/// For a multi-word rendering, oblique forms mostly *append* to the last token,
/// so a plain substring test on the nominative already catches them. For a single
/// word we also match any token that *starts with* the stem and is only a short
/// suffix longer — this catches declensions ("Аню") without matching unrelated
/// words that merely contain the stem ("странно").
pub fn paragraph_mentions(paragraph: &str, old_target: &str) -> bool {
    let old = old_target.trim().to_lowercase();
    if old.is_empty() {
        return false;
    }
    let hay = paragraph.to_lowercase();
    if hay.contains(&old) {
        return true;
    }
    // single-word stem / word-prefix matching
    if !old.contains(char::is_whitespace) {
        let st = stem(&old);
        if st.chars().count() >= 2 {
            let st_len = st.chars().count();
            return hay
                .split(|c: char| !c.is_alphabetic())
                .filter(|w| !w.is_empty())
                .any(|w| {
                    let wl = w.chars().count();
                    w.starts_with(&st) && wl <= st_len + 3
                });
        }
    }
    false
}

/// Build the (system, user) prompt to rewrite one paragraph, replacing every
/// form of `old_target` with the correctly inflected form of `new_target` and
/// fixing gender/number/case agreement of the surrounding words.
pub fn rewrite_prompt(
    target_lang: &str,
    kind: &str,
    old_target: &str,
    new_target: &str,
    paragraph: &str,
) -> (String, String) {
    let system = format!(
        "You are editing an existing {target_lang} translation of a book. In the paragraph below, a {kind} that was previously rendered as \"{old_target}\" must now be rendered as \"{new_target}\".\n\
         Replace EVERY mention of it, including all inflected/declined forms, with the correct grammatical form of \"{new_target}\". Adjust the surrounding words so that grammatical gender, number and case agree (for example past-tense verbs and adjectives that refer to it). Do not translate, rephrase, or change anything else — keep all other wording identical.\n\
         Output only the corrected paragraph: no quotes, no notes, no explanations."
    );
    (system, paragraph.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nominative_and_oblique() {
        assert!(paragraph_mentions("Сюй Цин улыбнулся.", "Сюй Цин"));
        // genitive appends to the last token → nominative is a substring
        assert!(paragraph_mentions("Он не видел Сюй Цина уже год.", "Сюй Цин"));
        // single feminine name via stem
        assert!(paragraph_mentions("Он позвал Аню домой.", "Аня"));
        assert!(paragraph_mentions("У Ани был меч.", "Аня"));
    }

    #[test]
    fn ignores_unrelated_paragraphs() {
        assert!(!paragraph_mentions("Совсем другой текст.", "Сюй Цин"));
        assert!(!paragraph_mentions("", "Аня"));
        assert!(!paragraph_mentions("что-то", ""));
    }

    #[test]
    fn prompt_mentions_both_renderings() {
        let (sys, user) = rewrite_prompt("Russian", "person", "Сюй Цин", "Иван", "Сюй Цин ушёл.");
        assert!(sys.contains("Сюй Цин") && sys.contains("Иван"));
        assert_eq!(user, "Сюй Цин ушёл.");
    }
}
