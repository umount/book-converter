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


/// Writing system a language is expected to be written in. Used to catch text
/// the model left in the wrong language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Cyrillic,
    Han,
}

/// The script a translation into `lang` should be written in, or `None` for
/// languages this check cannot judge (mixed writing systems such as Japanese).
pub fn expected_script(lang: &str) -> Option<Script> {
    match lang.trim().to_ascii_lowercase().as_str() {
        "russian" | "ukrainian" | "belarusian" | "bulgarian" | "serbian" => Some(Script::Cyrillic),
        "english" | "german" | "french" | "spanish" | "italian" | "portuguese" | "dutch"
        | "polish" | "czech" | "turkish" | "vietnamese" | "indonesian" => Some(Script::Latin),
        "chinese" => Some(Script::Han),
        // Japanese and Korean mix scripts (kana + kanji, hangul + hanja); Arabic,
        // Hebrew, Greek and friends are simply not modelled here.
        _ => None,
    }
}

fn script_of(c: char) -> Option<Script> {
    match c {
        'a'..='z' | 'A'..='Z' | 'À'..='ÿ' => Some(Script::Latin),
        'А'..='я' | 'Ё' | 'ё' | 'Ї' | 'ї' | 'І' | 'і' | 'Є' | 'є' | 'Ґ' | 'ґ' => Some(Script::Cyrillic),
        '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' => Some(Script::Han),
        _ => None,
    }
}

/// Runs of text in a script other than `expected`, i.e. words the model failed to
/// translate (source-script names) or pulled in from a third language.
///
/// A run is reported when it is at least `min_len` characters long, so stray
/// single letters (a unit, an initial) do not trip it. Anything that appears
/// verbatim in `source` is allowed: if the original itself carried a Latin word,
/// keeping it is correct.
pub fn foreign_fragments(
    text: &str,
    expected: Script,
    source: &str,
    min_len: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut run = String::new();
    let mut run_script: Option<Script> = None;

    let flush = |run: &mut String, script: Option<Script>, out: &mut Vec<String>| {
        let word = run.trim().to_string();
        run.clear();
        let Some(script) = script else { return };
        if script == expected || word.is_empty() {
            return;
        }
        // Han in a non-Han target is untranslated source text, however short and
        // even though it does appear in the original - that is exactly the point.
        let is_source_script = script == Script::Han;
        let long_enough = is_source_script || word.chars().count() >= min_len;
        let carried_over = !is_source_script && source.contains(&word);
        if long_enough && !carried_over && !out.contains(&word) {
            out.push(word);
        }
    };

    for c in text.chars() {
        match script_of(c) {
            Some(s) if Some(s) == run_script => run.push(c),
            Some(s) => {
                flush(&mut run, run_script, &mut out);
                run_script = Some(s);
                run.push(c);
            }
            // Apostrophes and hyphens stay inside a word ("Bai'er", "Wang-shi").
            None if !run.is_empty() && matches!(c, '\'' | '’' | '-') => run.push(c),
            None => {
                flush(&mut run, run_script, &mut out);
                run_script = None;
            }
        }
    }
    flush(&mut run, run_script, &mut out);
    out
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

    #[test]
    fn flags_untranslated_han_and_latin_in_russian() {
        let out = foreign_fragments(
            "Он посмотрел на 王林 и сказал cultivation.",
            Script::Cyrillic,
            "他看着王林",
            2,
        );
        assert!(out.contains(&"cultivation".to_string()), "got: {out:?}");
        // 王林 is in the source, but Han is never acceptable in a Cyrillic target.
        assert!(out.contains(&"王林".to_string()), "got: {out:?}");
    }

    #[test]
    fn allows_latin_that_was_in_the_source() {
        let out = foreign_fragments("Он включил Wi-Fi.", Script::Cyrillic, "他打开了 Wi-Fi。", 2);
        assert!(out.is_empty(), "got: {out:?}");
    }

    #[test]
    fn ignores_single_letters_and_clean_text() {
        assert!(foreign_fragments("Чистый русский текст.", Script::Cyrillic, "", 2).is_empty());
        assert!(foreign_fragments("Пункт a) первый", Script::Cyrillic, "", 2).is_empty());
    }

    #[test]
    fn expected_script_maps_known_languages() {
        assert_eq!(expected_script("Russian"), Some(Script::Cyrillic));
        assert_eq!(expected_script("english"), Some(Script::Latin));
        assert_eq!(expected_script("Chinese"), Some(Script::Han));
        assert_eq!(expected_script("Japanese"), None);
    }
}
