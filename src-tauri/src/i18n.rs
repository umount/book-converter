//! Localization of output-facing strings (e.g. the "Contents" heading, the
//! "Chapter" label). Strings live in `assets/locales.json` — add a language by
//! adding a top-level entry there; no code changes needed.

use std::collections::HashMap;
use std::sync::OnceLock;

type Locales = HashMap<String, HashMap<String, String>>;

fn locales() -> &'static Locales {
    static LOCALES: OnceLock<Locales> = OnceLock::new();
    LOCALES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/locales.json")).unwrap_or_default()
    })
}

/// Normalize a language name or code to a short code (`ru`, `en`, `zh`, …).
pub fn lang_code(lang: &str) -> String {
    let l = lang.trim().to_lowercase();
    if l == "ru" || l.starts_with("rus") || l.contains("рус") {
        "ru".into()
    } else if l == "en" || l.starts_with("eng") || l.contains("англ") {
        "en".into()
    } else if l == "zh" || l.starts_with("chi") || l.contains("кит") {
        "zh".into()
    } else if l.len() >= 2 {
        l.chars().take(2).collect()
    } else {
        "en".into()
    }
}

/// A localized label for `key` in the given target language, falling back to
/// English and then to the key itself.
pub fn label(lang: &str, key: &str) -> String {
    let code = lang_code(lang);
    let l = locales();
    l.get(&code)
        .and_then(|m| m.get(key))
        .or_else(|| l.get("en").and_then(|m| m.get(key)))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_codes() {
        assert_eq!(lang_code("Russian"), "ru");
        assert_eq!(lang_code("русский"), "ru");
        assert_eq!(lang_code("English"), "en");
        assert_eq!(lang_code("Chinese"), "zh");
        assert_eq!(lang_code("zh"), "zh");
    }

    #[test]
    fn labels_and_fallback() {
        assert_eq!(label("Russian", "contents"), "Содержание");
        assert_eq!(label("English", "contents"), "Contents");
        // unknown key → the key itself
        assert_eq!(label("Russian", "nope"), "nope");
        // unknown language → English fallback
        assert_eq!(label("Klingon", "contents"), "Contents");
    }
}
