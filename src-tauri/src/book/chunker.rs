//! Split a chapter into chunks that fit a DeepSeek request limit.
//!
//! The base unit of translation is a chapter (~4300 Chinese chars on average,
//! fits in one request). This module is only a fallback: if a chapter is
//! abnormally long (> max_chunk_chars), split it on paragraph boundaries,
//! NEVER mid-sentence.

use super::parser::Chapter;

/// A part of a chapter sent as a single request.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Ordinal number of the chunk within the chapter (0-based); the first chunk
    /// carries the title, and the orchestrator rejoins parts with a blank line.
    pub part: usize,
    /// Chunk text.
    pub text: String,
}

/// Split a chapter into chunks no longer than `max_chunk_chars` characters.
///
/// If the chapter fits whole, returns a single `Chunk`.
/// Otherwise splits on paragraph boundaries (`\n\n`, then `\n` as a fallback),
/// never mid-sentence. A single paragraph longer than the limit is kept intact
/// as its own chunk (still no mid-sentence cut).
pub fn split_chapter(chapter: &Chapter, max_chunk_chars: usize) -> Vec<Chunk> {
    let body = chapter.body.trim();
    if body.is_empty() {
        return vec![Chunk {
            part: 0,
            text: String::new(),
        }];
    }
    if max_chunk_chars == 0 || char_len(body) <= max_chunk_chars {
        return vec![Chunk {
            part: 0,
            text: body.to_string(),
        }];
    }

    let paragraphs = split_paragraphs(body);
    let texts = pack_paragraphs(&paragraphs, max_chunk_chars);
    texts
        .into_iter()
        .enumerate()
        .map(|(part, text)| Chunk { part, text })
        .collect()
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Prefer blank-line paragraphs; if that yields a single block, fall back to
/// single newlines (common in scraped novels).
fn split_paragraphs(body: &str) -> Vec<&str> {
    let blank: Vec<&str> = body
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if blank.len() >= 2 {
        return blank;
    }
    let lines: Vec<&str> = body
        .split('\n')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if lines.len() >= 2 {
        lines
    } else {
        vec![body]
    }
}

/// Greedily pack paragraphs into chunks ≤ `max_chunk_chars`. Oversized single
/// paragraphs become their own chunk (no mid-sentence split).
fn pack_paragraphs(paragraphs: &[&str], max_chunk_chars: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();

    for p in paragraphs {
        let p_len = char_len(p);
        if cur.is_empty() {
            cur.push_str(p);
            // Oversize alone: flush immediately so the next paragraph starts fresh.
            if p_len > max_chunk_chars {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // +2 for the blank line separator we rejoin with.
        let joined_len = char_len(&cur) + 2 + p_len;
        if joined_len <= max_chunk_chars {
            cur.push_str("\n\n");
            cur.push_str(p);
        } else {
            out.push(std::mem::take(&mut cur));
            cur.push_str(p);
            if p_len > max_chunk_chars {
                out.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(body: &str) -> Chapter {
        Chapter {
            index: 7,
            number: Some(7),
            title: "第七章".into(),
            body: body.into(),
        }
    }

    #[test]
    fn short_chapter_is_single_chunk() {
        let chunks = split_chapter(&ch("短正文。"), 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].part, 0);
        assert_eq!(chunks[0].text, "短正文。");
    }

    #[test]
    fn splits_on_blank_line_paragraphs() {
        let body = "第一段。\n\n第二段。\n\n第三段。";
        let chunks = split_chapter(&ch(body), 8);
        // Each CJK paragraph is 4 chars; max 8 → two paras per chunk when packing,
        // but "第一段。\n\n第二段。" is 4+2+4 = 10 > 8, so one per chunk.
        assert!(chunks.len() >= 2);
        let joined: String = chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(joined.contains("第一段"));
        assert!(joined.contains("第三段"));
        for c in &chunks {
            assert!(char_len(&c.text) <= 8 || c.text.chars().count() == 4);
        }
    }

    #[test]
    fn falls_back_to_single_newlines() {
        let body = "甲行\n乙行\n丙行\n丁行";
        let chunks = split_chapter(&ch(body), 5);
        assert!(chunks.len() >= 2);
        let all: String = chunks
            .iter()
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("|");
        assert!(all.contains("甲行"));
        assert!(all.contains("丁行"));
    }

    #[test]
    fn oversized_paragraph_kept_intact() {
        let long = "字".repeat(50);
        let body = format!("{long}\n\n短");
        let chunks = split_chapter(&ch(&body), 10);
        assert_eq!(chunks[0].text, long);
        assert!(chunks.last().unwrap().text.contains("短"));
    }

    #[test]
    fn empty_body() {
        let chunks = split_chapter(&ch("   "), 100);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.is_empty());
    }
}
