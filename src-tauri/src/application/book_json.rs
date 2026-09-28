//! Narrow repair for unescaped ASCII quotes inside balanced Russian dialogue quotes.
//! Only `text` values are edited. The caller must still deserialize and validate IDs.
pub(super) fn repair_dialogue_quotes(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut at = 0;
    let mut changed = false;
    while at < bytes.len() {
        if bytes[at] != b'"' {
            let c = input[at..].chars().next()?;
            out.push(c);
            at += c.len_utf8();
            continue;
        }
        // Read a normal JSON string (key, ID, or other value) unchanged.
        let start = at;
        at += 1;
        loop {
            match *bytes.get(at)? {
                b'\\' => at += 2,
                b'"' => { at += 1; break; }
                _ => at += 1,
            }
        }
        let token = &input[start..at];
        out.push_str(token);
        if token != "\"text\"" { continue; }
        let mut value = at;
        while bytes.get(value).is_some_and(u8::is_ascii_whitespace) { value += 1; }
        if bytes.get(value) != Some(&b':') { continue; }
        value += 1;
        while bytes.get(value).is_some_and(u8::is_ascii_whitespace) { value += 1; }
        if bytes.get(value) != Some(&b'"') { return None; }
        out.push_str(&input[at..=value]);
        at = value + 1;
        let mut depth = 0usize;
        loop {
            let c = input.get(at..)?.chars().next()?;
            at += c.len_utf8();
            match c {
                '\\' => {
                    out.push(c);
                    let escaped = input.get(at..)?.chars().next()?;
                    out.push(escaped);
                    at += escaped.len_utf8();
                }
                '«' => { depth += 1; out.push(c); }
                '»' => { depth = depth.checked_sub(1)?; out.push(c); }
                '"' if depth > 0 => { out.push_str("\\\""); changed = true; }
                '"' => { out.push(c); break; }
                _ => out.push(c),
            }
        }
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repairs_only_quotes_inside_dialogue_and_preserves_words() {
        let raw = r#"{"segments":[{"id":"a","text":"«Что такое "лапает"?»\n«Одним словом — "напала нечисть"», — подумал он."}]}"#;
        let fixed = repair_dialogue_quotes(raw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(parsed["segments"][0]["text"], "«Что такое \"лапает\"?»\n«Одним словом — \"напала нечисть\"», — подумал он.");
    }
    #[test]
    fn leaves_escaped_quotes_and_other_fields_alone() {
        let valid = r#"{"segments":[{"id":"a","text":"«Слово \"да\"»."}]}"#;
        assert!(repair_dialogue_quotes(valid).is_none());
        for invalid in [
            r#"{"segments":[{"id":"a","text":"Слово "да"."}]}"#,
            r#"{"segments":[{"id":"a","text":"«Слово "да"."}]}"#,
            r#"{"segments":[{"id":"«a"b»","text":"test"}]}"#,
        ] {
            assert!(repair_dialogue_quotes(invalid).is_none_or(|s| serde_json::from_str::<serde_json::Value>(&s).is_err()));
        }
    }
}
