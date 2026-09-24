//! Prompt glossaries contain only literal matches, as in the original book pipeline.
use crate::storage::shared::GlossaryTerm;

pub fn payload(terms: &[GlossaryTerm], text: &str, translated: bool) -> String {
    let mut matches: Vec<_> = terms
        .iter()
        .filter(|term| {
            (!term.source.is_empty() && text.contains(&term.source))
                || (translated && !term.target.is_empty() && text.contains(&term.target))
        })
        .collect();
    matches.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.frequency.cmp(&a.frequency))
            .then(b.source.len().cmp(&a.source.len()))
            .then(a.source.cmp(&b.source))
    });
    serde_json::Value::Array(matches.into_iter().map(|t|serde_json::json!({"source":t.source,"target":t.target,"kind":t.kind,"pinned":t.pinned})).collect()).to_string()
}

#[cfg(test)]
pub(super) fn term(source: &str, target: &str) -> GlossaryTerm {
    GlossaryTerm {
        id: source.into(),
        source: source.into(),
        target: target.into(),
        kind: "term".into(),
        pinned: true,
        frequency: 1,
        revision: crate::app::contracts::Revision("0".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_pinned_terms_are_excluded_and_target_matches_only_apply_to_repairs() {
        let terms = vec![
            term("王林", "Ван Линь"),
            term("韩立", "Хань Ли"),
            term("", ""),
        ];
        let original: serde_json::Value =
            serde_json::from_str(&payload(&terms, "王林 пришёл", false)).unwrap();
        assert_eq!(original.as_array().unwrap().len(), 1);
        assert_eq!(original[0]["target"], "Ван Линь");
        assert!(original[0].get("revision").is_none());
        assert_eq!(payload(&terms, "Ван Линь", false), "[]");
        let repair: serde_json::Value =
            serde_json::from_str(&payload(&terms, "Ван Линь", true)).unwrap();
        assert_eq!(repair[0]["source"], "王林");
        assert_eq!(payload(&terms, "Без совпадений", true), "[]");
    }
}
