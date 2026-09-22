//! Detect which language a newly imported book is written in.
//!
//! Unique writing systems (Han, kana, hangul, Cyrillic) are decided locally
//! from a short sample. Latin-script languages look alike, so those fall
//! through to a small model call when an API key is present.

use crate::book::Chapter;
use crate::config::Config;
use crate::translator::DeepSeekClient;

/// Language names the translator prompts understand, matching the Settings list.
pub const TRANSLATION_LANGS: &[&str] = &[
    "English",
    "Russian",
    "Chinese",
    "Japanese",
    "Korean",
    "German",
    "French",
    "Spanish",
    "Italian",
    "Portuguese",
];

const SAMPLE_CHARS: usize = 1800;
const MIN_LETTERS: usize = 20;

/// Result of guessing the book's source language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedLang {
    pub name: String,
    /// False when the guess is the global Settings fallback, not a real detect.
    pub detected: bool,
}

/// Normalize a language name or code to a canonical prompt name.
pub fn canonical_lang(raw: &str) -> Option<&'static str> {
    let l = raw.trim().to_lowercase();
    if l.is_empty() {
        return None;
    }
    for name in TRANSLATION_LANGS {
        if name.eq_ignore_ascii_case(&l) {
            return Some(*name);
        }
    }
    Some(match l.as_str() {
        "zh" | "zh-cn" | "zh-hans" | "cn" | "中文" | "китайский" => "Chinese",
        "ja" | "jp" | "日本語" | "японский" => "Japanese",
        "ko" | "kr" | "한국어" | "한글" | "корейский" => "Korean",
        "ru" | "рус" | "русский" => "Russian",
        "en" | "eng" | "английский" => "English",
        "de" | "deutsch" | "немецкий" => "German",
        "fr" | "francais" | "français" | "французский" => "French",
        "es" | "espanol" | "español" | "испанский" => "Spanish",
        "it" | "italiano" | "итальянский" => "Italian",
        "pt" | "portugues" | "português" | "португальский" => "Portuguese",
        _ => return None,
    })
}

/// Opening excerpt used for detection: titles plus bodies, capped.
pub fn sample_book(chapters: &[Chapter]) -> String {
    sample_from_chapters(chapters, SAMPLE_CHARS)
}

fn sample_from_chapters(chapters: &[Chapter], max_chars: usize) -> String {
    let mut out = String::new();
    for chapter in chapters {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(chapter.title.trim());
        out.push('\n');
        out.push_str(chapter.body.trim());
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out.chars().take(max_chars).collect()
}

/// Guess from the writing system when the script identifies one listed language.
pub fn detect_from_script(text: &str) -> Option<&'static str> {
    let mut latin = 0usize;
    let mut cyrillic = 0usize;
    let mut han = 0usize;
    let mut kana = 0usize;
    let mut hangul = 0usize;
    for c in text.chars() {
        match script_kind(c) {
            Some(Kind::Latin) => latin += 1,
            Some(Kind::Cyrillic) => cyrillic += 1,
            Some(Kind::Han) => han += 1,
            Some(Kind::Kana) => kana += 1,
            Some(Kind::Hangul) => hangul += 1,
            None => {}
        }
    }
    let letters = latin + cyrillic + han + kana + hangul;
    if letters < MIN_LETTERS {
        return None;
    }
    // Kana never appears in Chinese prose; a handful is enough for Japanese.
    if kana >= 8 || (kana >= 3 && han > 0) {
        return Some("Japanese");
    }
    if hangul >= 8 {
        return Some("Korean");
    }
    let dominant = latin.max(cyrillic).max(han);
    if han == dominant && han * 2 >= letters {
        return Some("Chinese");
    }
    if cyrillic == dominant && cyrillic * 2 >= letters {
        return Some("Russian");
    }
    // Latin covers several listed languages; the model has to tell them apart.
    None
}

/// Heuristic first; a short model call only when the script is ambiguous.
pub async fn detect_source_lang(sample: &str, fallback: &str) -> DetectedLang {
    if let Some(name) = detect_from_script(sample) {
        return DetectedLang {
            name: name.to_string(),
            detected: true,
        };
    }
    if let Some(name) = detect_with_ai(sample).await {
        return DetectedLang {
            name: name.to_string(),
            detected: true,
        };
    }
    DetectedLang {
        name: canonical_lang(fallback).unwrap_or("Chinese").to_string(),
        detected: false,
    }
}

async fn detect_with_ai(sample: &str) -> Option<&'static str> {
    let sample = sample.trim();
    if sample.chars().count() < MIN_LETTERS {
        return None;
    }
    let config = Config::load();
    if !config.has_key() {
        return None;
    }
    let client = DeepSeekClient::new(config).ok()?;
    let allowed = TRANSLATION_LANGS.join(", ");
    let system = format!(
        "Identify the language of the book excerpt. \
         Reply with one JSON object: {{\"language\":\"<name>\"}}. \
         <name> must be exactly one of: {allowed}. \
         No other keys, no commentary."
    );
    let reply = client.translate_json(&system, sample).await.ok()?;
    parse_ai_language(&reply)
}

/// Pull a canonical language name out of a model JSON (or a bare name).
pub fn parse_ai_language(reply: &str) -> Option<&'static str> {
    let trimmed = reply.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(name) = value.get("language").and_then(|v| v.as_str()) {
            return canonical_lang(name);
        }
    }
    canonical_lang(trimmed.trim_matches(|c| c == '"' || c == '{' || c == '}'))
}

#[derive(Clone, Copy)]
enum Kind {
    Latin,
    Cyrillic,
    Han,
    Kana,
    Hangul,
}

fn script_kind(c: char) -> Option<Kind> {
    match c {
        'a'..='z' | 'A'..='Z' | 'À'..='ÿ' => Some(Kind::Latin),
        'А'..='я' | 'Ё' | 'ё' | 'Ї' | 'ї' | 'І' | 'і' | 'Є' | 'є' | 'Ґ' | 'ґ' => {
            Some(Kind::Cyrillic)
        }
        '\u{3040}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}' => Some(Kind::Kana),
        '\u{ac00}'..='\u{d7af}' | '\u{1100}'..='\u{11ff}' => Some(Kind::Hangul),
        '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' => Some(Kind::Han),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(title: &str, body: &str) -> Chapter {
        Chapter {
            index: 1,
            number: Some(1),
            title: title.into(),
            body: body.into(),
        }
    }

    #[test]
    fn canonical_names_and_codes() {
        assert_eq!(canonical_lang("japanese"), Some("Japanese"));
        assert_eq!(canonical_lang("zh"), Some("Chinese"));
        assert_eq!(canonical_lang("русский"), Some("Russian"));
        assert_eq!(canonical_lang("Deutsch"), Some("German"));
        assert_eq!(canonical_lang("klingon"), None);
        assert_eq!(canonical_lang("  "), None);
    }

    #[test]
    fn script_detects_unique_writing_systems() {
        let zh = "第一章 活着。他走进山谷，看着远处的山峰。风吹过树林。";
        assert_eq!(detect_from_script(zh), Some("Chinese"));

        let ja = "第一章 はじまり。彼は東京の街を歩いて、猫を見つけた。";
        assert_eq!(detect_from_script(ja), Some("Japanese"));

        let ko = "제1장 시작. 그는 서울 거리를 걸으며 고양이를 보았다.";
        assert_eq!(detect_from_script(ko), Some("Korean"));

        let ru = "Глава первая. Он вошёл в долину и посмотрел на горы вдали.";
        assert_eq!(detect_from_script(ru), Some("Russian"));

        let en = "Chapter one. He walked into the valley and looked at the mountains.";
        assert_eq!(detect_from_script(en), None);
    }

    #[test]
    fn too_short_or_punctuation_is_undecided() {
        assert_eq!(detect_from_script("... ???"), None);
        assert_eq!(detect_from_script("Hi."), None);
    }

    #[test]
    fn chinese_with_latin_terms_still_chinese() {
        let text = "他打开了 Wi-Fi，然后走进了山谷，看着远处的山峰和河流。";
        assert_eq!(detect_from_script(text), Some("Chinese"));
    }

    #[test]
    fn sample_caps_length() {
        let chapters = vec![chapter("T", &"字".repeat(5000))];
        let sample = sample_from_chapters(&chapters, 100);
        assert_eq!(sample.chars().count(), 100);
    }

    #[test]
    fn parse_ai_json_and_bare_name() {
        assert_eq!(
            parse_ai_language(r#"{"language":"French"}"#),
            Some("French")
        );
        assert_eq!(parse_ai_language("Spanish"), Some("Spanish"));
        assert_eq!(parse_ai_language(r#"{"language":"nope"}"#), None);
    }
}
