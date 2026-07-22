//! Small text helpers shared across translation context / retarget.

/// Closing excerpt of `text` for the next chapter's continuity prompt.
///
/// Takes complete sentences from the end until roughly `max_chars`. Never starts
/// mid-word or mid-sentence (falls back to a word boundary only when a single
/// final sentence is longer than `max_chars`).
pub fn closing_excerpt(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.is_empty() || max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let sentences = split_sentences(text);
    if sentences.is_empty() {
        return word_aware_tail(text, max_chars);
    }

    let mut picked: Vec<&str> = Vec::new();
    let mut len = 0usize;
    for sent in sentences.iter().rev() {
        let slen = sent.chars().count();
        if picked.is_empty() {
            if slen > max_chars {
                return word_aware_tail(sent.trim(), max_chars);
            }
            picked.push(sent.as_str());
            len = slen;
            continue;
        }
        if len + slen > max_chars {
            break;
        }
        picked.push(sent.as_str());
        len += slen;
    }
    picked.reverse();
    picked.concat().trim().to_string()
}

fn is_sentence_ender(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？')
}

fn is_closing_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '»' | '”' | '’' | ')' | ']')
}

/// Split `text` into sentences, keeping punctuation (and trailing quotes) on the
/// sentence they close. Paragraph breaks (`\n\n`) also start a new unit.
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        cur.push(c);

        let paragraph_break = c == '\n' && i + 1 < chars.len() && chars[i + 1] == '\n';
        if paragraph_break {
            while i + 1 < chars.len() && chars[i + 1] == '\n' {
                i += 1;
                cur.push(chars[i]);
            }
            out.push(std::mem::take(&mut cur));
            i += 1;
            continue;
        }

        if is_sentence_ender(c) {
            // Collapse ASCII "..." into one ending.
            if c == '.' {
                while i + 1 < chars.len() && chars[i + 1] == '.' {
                    i += 1;
                    cur.push(chars[i]);
                }
            }
            while i + 1 < chars.len() && is_closing_quote(chars[i + 1]) {
                i += 1;
                cur.push(chars[i]);
            }
            out.push(std::mem::take(&mut cur));
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Last `max_chars` of `text`, advanced to the next whitespace so we never start
/// mid-word.
fn word_aware_tail(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let rough = chars.len() - max_chars;
    let start = chars[rough..]
        .iter()
        .position(|c| c.is_whitespace())
        .map(|p| rough + p + 1)
        .unwrap_or(rough);
    chars[start..].iter().collect::<String>().trim_start().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_short_text() {
        assert_eq!(closing_excerpt("Короткий хвост.", 400), "Короткий хвост.");
    }

    #[test]
    fn snaps_to_sentence_not_mid_word() {
        let text = "Первое предложение тут. Второе предложение длиннее и интереснее. \
            Третье заканчивает главу гордостью.";
        let out = closing_excerpt(text, 80);
        assert!(
            out.starts_with("Второе") || out.starts_with("Третье"),
            "got: {out:?}"
        );
    }

    #[test]
    fn russian_mid_sentence_bug_regression() {
        let prefix = "А".repeat(50);
        let text = format!(
            "{prefix} был слегка распахнут, открывая шею. \
             В этот момент, услышав его слова, она произнесла холодным голосом."
        );
        let out = closing_excerpt(&text, 100);
        assert!(
            !out.starts_with("ыл "),
            "regression: mid-word cut; got: {out:?}"
        );
        assert!(
            out.starts_with("В этот момент"),
            "expected last full sentence; got: {out:?}"
        );
    }

    #[test]
    fn single_long_sentence_uses_word_boundary() {
        let text = format!("{} конец.", "слово ".repeat(80));
        let out = closing_excerpt(&text, 40);
        assert!(out.chars().count() <= 45);
        assert!(!out.starts_with("лово")); // not mid "слово"
    }
}
