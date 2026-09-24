//! Local candidate matching for bounded glossary corrections.

/// A stem of a single-word `target`: drop a trailing soft/vowel character that
/// inflection typically replaces ("Аня" → "ан", so it matches "Ани"/"Аню").
fn stem(word: &str) -> String {
    let mut chars: Vec<char> = word.trim().to_lowercase().chars().collect();
    if let Some(&last) = chars.last() {
        const TRIM: &[char] = &[
            'а', 'я', 'о', 'е', 'ы', 'и', 'й', 'ь', 'ю', 'э', 'a', 'e', 'o', 'y',
        ];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nominative_and_oblique() {
        assert!(paragraph_mentions("Сюй Цин улыбнулся.", "Сюй Цин"));
        // genitive appends to the last token → nominative is a substring
        assert!(paragraph_mentions(
            "Он не видел Сюй Цина уже год.",
            "Сюй Цин"
        ));
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
}
