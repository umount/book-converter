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
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

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
    /// Title from a `《...》` line, or the first non-label preamble line.
    pub title: Option<String>,
    /// Author from an `作者：...` line, e.g. "耳根".
    pub author: Option<String>,
    /// Source-language blurb from a `简介：` block, if the file includes one.
    pub summary: Option<String>,
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

/// Normalize line endings: CRLF and lone CR both become `\n`.
fn normalize_newlines(raw: &str) -> String {
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

/// One named chapter-heading pattern from the config.
#[derive(Debug, Clone, Deserialize)]
struct PatternDef {
    #[allow(dead_code)]
    name: String,
    pattern: String,
}

/// Chapter-heading patterns are data, not code: they live in
/// `assets/chapter_patterns.json`, so a new language is a config entry, not a code
/// change. Each is anchored to line start (`^` in multiline mode) so indented spam
/// and in-prose mentions are not mistaken for headers, and captures the chapter
/// number in group 1. Unknown layouts fall back to a model-inferred delimiter
/// (see [`build_delimiter_prompt`]).
fn pattern_defs() -> &'static Vec<PatternDef> {
    static DEFS: OnceLock<Vec<PatternDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/chapter_patterns.json"))
            .expect("chapter_patterns.json is valid")
    })
}

/// Compile the configured candidate patterns (skipping any that fail to compile).
pub fn candidate_patterns() -> Vec<Regex> {
    pattern_defs()
        .iter()
        .filter_map(|d| Regex::new(&d.pattern).ok())
        .collect()
}

/// Pick the chapter-heading pattern that matches the text best (most headers).
///
/// Returns `None` if no pattern matches at least twice — the caller can then ask
/// the model to infer a delimiter.
pub fn detect_chapter_pattern(text: &str) -> Option<Regex> {
    candidate_patterns()
        .into_iter()
        .map(|re| {
            let count = re.find_iter(text).count();
            (count, re)
        })
        .filter(|(count, _)| *count >= 2)
        .max_by_key(|(count, _)| *count)
        .map(|(_, re)| re)
}

/// Volume labels are derived from preserved source headings, including older imports.
/// Keep part and volume distinct; do not infer missing volume numbers.
pub fn chapter_volume(title: &str) -> Option<String> {
    static PREFIX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static UNIT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let prefix = PREFIX.get_or_init(|| Regex::new(r"^(第[0-9一二三四五六七八九十百千零两〇]+[部卷集].*?)(?:第[0-9一二三四五六七八九十百千零两〇]+[章回話话節节]|楔子|序章|序言|尾声|尾聲|后记|後記)").unwrap());
    let units = UNIT.get_or_init(|| Regex::new(r"第([0-9一二三四五六七八九十百千零两〇]+)([部卷集])").unwrap());
    let captures = prefix.captures(title)?;
    let mut labels = Vec::new();
    for unit in units.captures_iter(&captures[1]) {
        let number = parse_chapter_number(&unit[1])?;
        let label = format!("第{number}{}", &unit[2]);
        if !labels.contains(&label) { labels.push(label); }
    }
    (!labels.is_empty()).then(|| labels.join(" · "))
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

/// Split the raw book text into chapters, auto-detecting the heading pattern.
///
/// Returns an empty vector if no known pattern matches (the caller can then use a
/// model-inferred delimiter via [`parse_chapters_with`]). Text before the first
/// header (title, author, TOC) is treated as preamble — see [`parse_book_meta`].
pub fn parse_chapters(raw: &str) -> Vec<Chapter> {
    let text = normalize_newlines(raw);
    match detect_chapter_pattern(&text) {
        Some(re) => split_on_pattern(&text, &re),
        None => Vec::new(),
    }
}

/// Split already-normalized text using a specific heading pattern (group 1 = the
/// chapter number). Used by [`parse_chapters`] and by the model-inferred path.
fn split_on_pattern(text: &str, re: &Regex) -> Vec<Chapter> {
    // For each header line: (title_start, title_end, full_title, numeral).
    let headers: Vec<(usize, usize, String, Option<usize>)> = re
        .captures_iter(text)
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
    let preamble_end = detect_chapter_pattern(&text)
        .and_then(|re| re.find(&text).map(|m| m.start()))
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
        title: first_group(r"《(.+?)》").or_else(|| {
            preamble
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !is_preamble_label(line))
                .map(str::to_string)
        }),
        author: first_group(r"作者[:：]\s*(.+)"),
        summary: extract_blurb(preamble),
        declared_chapters: first_group(r"总章节数[:：]\s*(\d+)").and_then(|s| s.parse().ok()),
    }
}

/// Preamble lines that are metadata labels, not the book title.
fn is_preamble_label(line: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^(?:作者|简介|簡介|总章节数|總章節數)[:：]|^第.+卷\s*$")
            .expect("preamble label regex")
    });
    re.is_match(line)
}

/// Collect the `简介：` block until a blank line or the next label/volume header.
fn extract_blurb(preamble: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"(?m)^(?:简介|簡介)[:：]\s*(.*)$").expect("blurb regex"));
    let caps = re.captures(preamble)?;
    let mut parts = Vec::new();
    if let Some(first) = caps
        .get(1)
        .map(|m| m.as_str().trim())
        .filter(|s| !s.is_empty())
    {
        parts.push(first.to_string());
    }
    let after = caps.get(0)?.end();
    let rest = preamble[after..].trim_start_matches(['\r', '\n']);
    for line in rest.lines() {
        let t = line.trim();
        if t.is_empty() || is_preamble_label(t) {
            break;
        }
        parts.push(t.to_string());
    }
    let text = parts.join("\n");
    if text.is_empty() {
        None
    } else {
        Some(text)
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
    for chapter in chapters {
        if let Some(n) = chapter.number {
            if !seen.insert((chapter_volume(&chapter.title), n)) {
                dups.insert(n);
            }
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
        assert!(meta.summary.is_none());
    }

    #[test]
    fn parses_plain_title_author_and_blurb() {
        let text = "苟在诸天从黑暗佛门开始\n作者: 是桃花酥呀\n简介：\n天下之欲皆在香火，\n庸庸来客成我极乐，\n李玄一头栽入诸天世界。\n\n第1卷\n\n1.极乐\nbody\n";
        let meta = parse_book_meta(text);
        assert_eq!(meta.title.as_deref(), Some("苟在诸天从黑暗佛门开始"));
        assert_eq!(meta.author.as_deref(), Some("是桃花酥呀"));
        let blurb = meta.summary.as_deref().unwrap();
        assert!(blurb.contains("天下之欲皆在香火"));
        assert!(blurb.contains("李玄一头栽入诸天世界。"));
        assert!(!blurb.contains("第1卷"));
        assert!(!blurb.contains("1.极乐"));
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
        let src =
            "《光阴之外》作者：耳根\n\n第1章 活着\n\n　　body one\n\n第2章 异质\n　　body two\n";
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
        assert!(!report.missing_numbers.is_empty());
    }

    #[test]
    fn detects_english_chapters() {
        let text = "My Book\n\nChapter 1\nHe woke up.\n\nChapter 2\nThe end.\n";
        let chapters = parse_chapters(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].number, Some(1));
        assert_eq!(chapters[0].title, "Chapter 1");
        assert_eq!(chapters[0].body, "He woke up.");
    }

    #[test]
    fn detects_russian_chapters() {
        let text = "Книга\n\nГлава 1\nТекст.\n\nГлава 2\nЕщё текст.\n";
        let chapters = parse_chapters(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[1].number, Some(2));
        assert_eq!(chapters[1].title, "Глава 2");
    }

    #[test]
    fn detects_numbered_dot_headings() {
        let text = "书名\n作者: x\n\n第1卷\n\n1.极乐\n　　body one\n\n2.我...是谁？\n　　body two\n\n47. 有空格\n　　body three\n";
        let chapters = parse_chapters(text);
        assert_eq!(chapters.len(), 3, "{chapters:?}");
        assert_eq!(chapters[0].number, Some(1));
        assert_eq!(chapters[0].title, "1.极乐");
        assert_eq!(chapters[0].body, "body one");
        assert_eq!(chapters[2].number, Some(47));
        assert_eq!(chapters[2].title, "47. 有空格");
    }

    #[test]
    fn no_pattern_returns_empty() {
        let text = "Just prose with no chapter headings at all. One line.\n";
        assert!(parse_chapters(text).is_empty());
        assert!(detect_chapter_pattern(text).is_none());
    }


}

#[cfg(test)]
mod volume_tests {
    use super::*;
    #[test]
    fn splits_compound_headings_without_losing_restarted_numbers_or_prologues() {
        let text = "Книга\n第01集 第一章 古音\n正文包含第01集第一章古音，不是标题。\n第01集 第二章 标题\n正文二\n第二部 楔子\n序言正文\n第二部第七集 第一章 道途\n正文三\n第二部 第十九集 尘埃落定 第八章 终章\n结尾";
        let chapters = parse_chapters(text);
        assert_eq!(chapters.len(), 5);
        assert_eq!(chapters.iter().map(|c| c.number).collect::<Vec<_>>(), vec![Some(1), Some(2), None, Some(1), Some(8)]);
        assert_eq!(chapters[0].body, "正文包含第01集第一章古音，不是标题。");
        assert_eq!(chapters[2].body, "序言正文");
        assert_eq!(chapters[4].body, "结尾");
        assert!(validate(&chapters, &BookMeta::default()).duplicate_numbers.is_empty());
        assert_eq!(chapter_volume(&chapters[0].title).as_deref(), Some("第1集"));
        assert_eq!(chapter_volume(&chapters[2].title).as_deref(), Some("第2部"));
        assert_eq!(chapter_volume(&chapters[3].title).as_deref(), Some("第2部 · 第7集"));
    }
    #[test]
    fn normalizes_volume_numbers_and_duplicate_prefixes_without_guessing() {
        assert_eq!(chapter_volume("第二部 第十七集 第十七集仙路杀劫 第一章 大战"), chapter_volume("第二部第17集 第二章 继续"));
        assert_eq!(chapter_volume("第二部 第一章 逃命").as_deref(), Some("第2部"));
        assert_eq!(chapter_volume("第一章 标题"), None);
        assert_eq!(parse_chapters("第1卷 第一章 开始\n一\n第二章 继续\n二").len(), 2);
    }
}
