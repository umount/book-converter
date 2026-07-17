//! Parse the source `.txt` into chapters by markers like `第一章 ...`.
//!
//! The book `光阴之外` has ~990 chapters; boundaries are given by a line that
//! starts with "第N章 Title" (N is a Chinese numeral). Line endings may be mixed
//! CRLF/CR — they are normalized to `\n` before parsing.
//!
//! Scraped web novels are messy: this source has 6 duplicated chapter blocks,
//! ~16 chapters missing outright, and indented spam copies of headers
//! ("…免费阅读"). The parser stays faithful (it extracts real, line-start headers
//! only) and exposes [`validate`] so the UI can warn about gaps and duplicates
//! instead of silently producing a book with holes.
//!
//! This module is intentionally self-contained (only `regex` + std) so it can be
//! compiled and tested without the rest of the Tauri crate.

use std::collections::BTreeSet;

use regex::Regex;

/// A single chapter of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    /// Sequential position (1-based) in reading order. Stable key for the
    /// progress store and output ordering, even when source numbering has gaps.
    pub index: usize,
    /// Chapter number parsed from the Chinese numeral in the title, if it could
    /// be read (e.g. 984 for "第九百八十四章"). Used for gap/duplicate detection.
    pub number: Option<usize>,
    /// Chapter title line, e.g. "第一章 活着".
    pub title: String,
    /// Full chapter text (without the title line), trimmed.
    pub body: String,
}

/// Book-level metadata parsed from the preamble before the first chapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookMeta {
    /// Title from a `《...》` line, e.g. "光阴之外".
    pub title: Option<String>,
    /// Author from an `作者：...` line, e.g. "耳根".
    pub author: Option<String>,
    /// Chapter count the source declares (`总章节数：990`), if present.
    pub declared_chapters: Option<usize>,
}

/// Data-quality report for a parsed book. Lets the UI warn before a long run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseReport {
    /// Chapters actually extracted.
    pub parsed: usize,
    /// Chapters the source claims to have, if declared.
    pub declared: Option<usize>,
    /// Highest chapter number seen.
    pub max_number: Option<usize>,
    /// Chapter numbers absent within `1..=max_number`.
    pub missing_numbers: Vec<usize>,
    /// Chapter numbers that appear more than once.
    pub duplicate_numbers: Vec<usize>,
}

impl ParseReport {
    /// True when the source is complete and free of duplicates.
    pub fn is_clean(&self) -> bool {
        self.missing_numbers.is_empty()
            && self.duplicate_numbers.is_empty()
            && self.declared.map_or(true, |d| d == self.parsed)
    }
}

/// Normalize line endings: CRLF and lone CR both become `\n`.
fn normalize_newlines(raw: &str) -> String {
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

/// Regex matching a chapter header at the start of a line: `第<numeral>章<rest>`.
///
/// The numeral is either Arabic (`第1章`, used by the complete edition) or a
/// Chinese numeral (`第一章`, used by an older edition). Anchored to line start
/// (`^` in multiline mode) so indented spam copies and in-prose mentions of a
/// chapter are not mistaken for real headers.
fn chapter_header_regex() -> Regex {
    // Chinese numerals cover 一..九, 十/百/千 places, plus 零/两/〇 seen in such novels.
    Regex::new(r"(?m)^第([0-9]+|[一二三四五六七八九十百千零两〇]+)章[^\n]*")
        .expect("chapter header regex is valid")
}

/// Parse a chapter number from either an Arabic (`123`) or Chinese (`一百二十三`)
/// numeral.
pub fn parse_chapter_number(numeral: &str) -> Option<usize> {
    if numeral.bytes().all(|b| b.is_ascii_digit()) {
        numeral.parse().ok()
    } else {
        cn_numeral_to_int(numeral)
    }
}

/// Convert a Chinese numeral (up to the 千 place) to an integer.
///
/// Handles forms like 一, 十, 十一, 二十, 一百一十八, 九百八十四, 一千零一.
/// Returns `None` for an empty/unreadable string. Values above 9999 (the 万
/// place) are out of scope — chapter counts do not reach them here.
fn cn_numeral_to_int(s: &str) -> Option<usize> {
    // Note: Chinese digits are NOT contiguous in Unicode (一=U+4E00, 二=U+4E8C,
    // 三=U+4E09 …), so a range check like '一'..='九' is wrong. Each arm sets
    // `seen` explicitly instead.
    let mut result: usize = 0;
    let mut current: usize = 0;
    let mut seen = false;

    for ch in s.chars() {
        match ch {
            '零' | '〇' => {}
            '一' => current = 1,
            '二' | '两' => current = 2,
            '三' => current = 3,
            '四' => current = 4,
            '五' => current = 5,
            '六' => current = 6,
            '七' => current = 7,
            '八' => current = 8,
            '九' => current = 9,
            '十' => {
                result += if current == 0 { 10 } else { current * 10 };
                current = 0;
            }
            '百' => {
                result += if current == 0 { 100 } else { current * 100 };
                current = 0;
            }
            '千' => {
                result += if current == 0 { 1000 } else { current * 1000 };
                current = 0;
            }
            _ => return None,
        }
        seen = true;
    }

    let total = result + current;
    if seen && total > 0 {
        Some(total)
    } else {
        None
    }
}

/// Split the raw book text into chapters.
///
/// Text before the first chapter marker (title, author, TOC line) is treated as
/// preamble and dropped from the chapter list — see [`parse_book_meta`].
pub fn parse_chapters(raw: &str) -> Vec<Chapter> {
    let text = normalize_newlines(raw);
    let re = chapter_header_regex();

    // For each header line: (title_start, title_end, full_title, numeral).
    let headers: Vec<(usize, usize, String, Option<usize>)> = re
        .captures_iter(&text)
        .map(|c| {
            let m = c.get(0).expect("group 0 always present");
            let numeral = c.get(1).and_then(|n| parse_chapter_number(n.as_str()));
            (m.start(), m.end(), m.as_str().trim().to_string(), numeral)
        })
        .collect();

    headers
        .iter()
        .enumerate()
        .map(|(i, &(_, title_end, ref title, number))| {
            // Body runs from the end of this header line to the start of the next.
            let body_end = headers
                .get(i + 1)
                .map(|&(next_start, ..)| next_start)
                .unwrap_or(text.len());
            let body = text[title_end..body_end].trim().to_string();
            Chapter {
                index: i + 1,
                number,
                title: title.clone(),
                body,
            }
        })
        .collect()
}

/// Parse book metadata from the preamble (everything before the first chapter).
pub fn parse_book_meta(raw: &str) -> BookMeta {
    let text = normalize_newlines(raw);

    // Limit search to the preamble so a `《...》` inside prose is not picked up.
    let preamble_end = chapter_header_regex()
        .find(&text)
        .map(|m| m.start())
        .unwrap_or(text.len());
    let preamble = &text[..preamble_end];

    let first_group = |pattern: &str| -> Option<String> {
        Regex::new(pattern)
            .ok()?
            .captures(preamble)?
            .get(1)
            .map(|g| g.as_str().trim().to_string())
    };

    BookMeta {
        title: first_group(r"《(.+?)》"),
        author: first_group(r"作者[:：]\s*(.+)"),
        declared_chapters: first_group(r"总章节数[:：]\s*(\d+)").and_then(|s| s.parse().ok()),
    }
}

/// Cross-check parsed chapters against declared count and internal numbering.
pub fn validate(chapters: &[Chapter], meta: &BookMeta) -> ParseReport {
    let numbers: Vec<usize> = chapters.iter().filter_map(|c| c.number).collect();
    let max_number = numbers.iter().copied().max();

    let present: BTreeSet<usize> = numbers.iter().copied().collect();
    let missing_numbers = match max_number {
        Some(max) => (1..=max).filter(|n| !present.contains(n)).collect(),
        None => Vec::new(),
    };

    // A number is a duplicate if it occurs more than once.
    let mut seen = BTreeSet::new();
    let mut dups = BTreeSet::new();
    for n in numbers {
        if !seen.insert(n) {
            dups.insert(n);
        }
    }

    ParseReport {
        parsed: chapters.len(),
        declared: meta.declared_chapters,
        max_number,
        missing_numbers,
        duplicate_numbers: dups.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "《光阴之外》\r\n作者：耳根\r\n总章节数：3\r\n\r\n第一章 活着\r\n\r\nbody one\r\nline two\r\n\r\n第二章 异质\r\nbody two\r\n\r\n第十章 跳号\r\nbody ten\r\n";

    #[test]
    fn parses_all_chapters() {
        let chapters = parse_chapters(SAMPLE);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].index, 1);
        assert_eq!(chapters[0].number, Some(1));
        assert_eq!(chapters[0].title, "第一章 活着");
        assert_eq!(chapters[0].body, "body one\nline two");
        assert_eq!(chapters[2].title, "第十章 跳号");
        assert_eq!(chapters[2].number, Some(10));
    }

    #[test]
    fn preamble_is_not_a_chapter() {
        let chapters = parse_chapters(SAMPLE);
        assert!(!chapters[0].body.contains("作者"));
    }

    #[test]
    fn normalizes_crlf() {
        let chapters = parse_chapters(SAMPLE);
        assert!(!chapters.iter().any(|c| c.body.contains('\r')));
    }

    #[test]
    fn parses_metadata() {
        let meta = parse_book_meta(SAMPLE);
        assert_eq!(meta.title.as_deref(), Some("光阴之外"));
        assert_eq!(meta.author.as_deref(), Some("耳根"));
        assert_eq!(meta.declared_chapters, Some(3));
    }

    #[test]
    fn chinese_numerals() {
        assert_eq!(cn_numeral_to_int("一"), Some(1));
        assert_eq!(cn_numeral_to_int("十"), Some(10));
        assert_eq!(cn_numeral_to_int("十一"), Some(11));
        assert_eq!(cn_numeral_to_int("二十"), Some(20));
        assert_eq!(cn_numeral_to_int("一百一十八"), Some(118));
        assert_eq!(cn_numeral_to_int("九百八十四"), Some(984));
        assert_eq!(cn_numeral_to_int("两百"), Some(200));
        assert_eq!(cn_numeral_to_int("一千零一"), Some(1001));
        assert_eq!(cn_numeral_to_int(""), None);
    }

    #[test]
    fn parses_arabic_numeral_headers() {
        // The complete edition uses Arabic numerals and full-width indentation.
        let src = "《光阴之外》作者：耳根\n\n第1章 活着\n\n　　body one\n\n第2章 异质\n　　body two\n";
        let chapters = parse_chapters(src);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].number, Some(1));
        assert_eq!(chapters[0].title, "第1章 活着");
        assert_eq!(chapters[1].number, Some(2));
    }

    #[test]
    fn validation_flags_gaps() {
        // Chapters 1, 2, 10 → missing 3..=9, no duplicates.
        let chapters = parse_chapters(SAMPLE);
        let meta = parse_book_meta(SAMPLE);
        let report = validate(&chapters, &meta);
        assert_eq!(report.parsed, 3);
        assert_eq!(report.declared, Some(3));
        assert_eq!(report.max_number, Some(10));
        assert_eq!(report.missing_numbers, vec![3, 4, 5, 6, 7, 8, 9]);
        assert!(report.duplicate_numbers.is_empty());
        assert!(!report.is_clean());
    }
}
