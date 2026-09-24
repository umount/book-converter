//! Structural FB2 import. Keep all sections and every image occurrence in order.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use base64::Engine as _;
use quick_xml::{events::Event, Reader};

use super::{
    blocks::{derive_text, Block, ChapterBlocks, ChapterKind},
    fb2::heading_number,
    parser::Chapter,
};

#[derive(Default)]
pub struct Content {
    pub chapters: Vec<Chapter>,
    pub blocks: Vec<ChapterBlocks>,
    pub images: Vec<(String, Vec<u8>)>,
}

fn flush(text: &mut String, blocks: &mut Vec<Block>) {
    if !text.trim().is_empty() {
        blocks.push(Block::text(text.trim()));
    }
    text.clear();
}

fn finish(content: &mut Content, title: &str, blocks: &mut Vec<Block>) {
    if blocks.is_empty() && title.trim().is_empty() {
        return;
    }
    let index = content.chapters.len() + 1;
    let title = if title.trim().is_empty() {
        index.to_string()
    } else {
        title.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    content.chapters.push(Chapter {
        index,
        number: heading_number(&title).map(|(n, _)| n),
        title,
        body: derive_text(blocks),
    });
    content.blocks.push(ChapterBlocks {
        chapter_index: index,
        kind: ChapterKind::classify(blocks),
        blocks: std::mem::take(blocks),
    });
}

pub fn parse(xml: &str) -> Result<Content> {
    let mut content = Content::default();
    let mut reader = Reader::from_str(xml);
    let (mut in_body, mut depth, mut title_depth) = (false, 0usize, 0usize);
    let (mut title, mut text, mut blocks) = (String::new(), String::new(), Vec::new());
    loop {
        match reader.read_event().context("reading FB2 content")? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"body" => in_body = true,
                b"section" if in_body => {
                    flush(&mut text, &mut blocks);
                    if depth == 0 {
                        finish(&mut content, &title, &mut blocks);
                        title.clear();
                    }
                    depth += 1;
                }
                b"title" if in_body => {
                    flush(&mut text, &mut blocks);
                    title_depth += 1;
                }
                b"image" if in_body => image(&e, &mut text, &mut blocks)?,
                _ => {}
            },
            Event::Empty(e) if in_body => match e.local_name().as_ref() {
                b"image" => image(&e, &mut text, &mut blocks)?,
                b"empty-line" => flush(&mut text, &mut blocks),
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                b"section" if in_body => {
                    flush(&mut text, &mut blocks);
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        finish(&mut content, &title, &mut blocks);
                        title.clear();
                    }
                }
                b"body" => {
                    flush(&mut text, &mut blocks);
                    finish(&mut content, &title, &mut blocks);
                    title.clear();
                    in_body = false;
                }
                b"title" if in_body => {
                    title_depth = title_depth.saturating_sub(1);
                    flush(&mut text, &mut blocks);
                }
                b"p" | b"v" | b"subtitle" | b"text-author" if in_body => {
                    if title_depth > 0 && depth <= 1 {
                        title.push(' ');
                    } else {
                        flush(&mut text, &mut blocks);
                    }
                }
                _ => {}
            },
            Event::Text(e) if in_body => {
                let value = e.unescape()?;
                if title_depth > 0 && depth <= 1 {
                    title.push_str(&value);
                } else {
                    text.push_str(&value);
                }
            }
            Event::CData(e) if in_body => {
                let value = std::str::from_utf8(e.as_ref())?;
                if title_depth > 0 && depth <= 1 {
                    title.push_str(value);
                } else {
                    text.push_str(value);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let required: HashSet<String> = content
        .blocks
        .iter()
        .flat_map(|c| &c.blocks)
        .filter_map(|b| b.asset_id.clone())
        .collect();
    let mut found = HashMap::new();
    let mut total = 0usize;
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.local_name().as_ref() == b"binary" => {
                let mut id = None;
                for attribute in e.attributes() {
                    let attribute = attribute?;
                    if attribute.key.as_ref() == b"id" {
                        id = Some(attribute.unescape_value()?.into_owned());
                    }
                }
                if let Some(id) = id.filter(|id| required.contains(id)) {
                    anyhow::ensure!(!found.contains_key(&id), "duplicate FB2 image id");
                    let encoded = reader.read_text(e.name())?;
                    anyhow::ensure!(encoded.len() <= 32 * 1024 * 1024, "FB2 image is too large");
                    let encoded: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .context("decoding FB2 image")?;
                    anyhow::ensure!(bytes.len() <= 20 * 1024 * 1024, "FB2 image is too large");
                    total += bytes.len();
                    anyhow::ensure!(total <= 256 * 1024 * 1024, "FB2 images exceed import limit");
                    found.insert(id, bytes);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    anyhow::ensure!(found.len() == required.len(), "missing FB2 image binary");
    content.images = found.into_iter().collect();
    content.images.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(content)
}

fn image(
    e: &quick_xml::events::BytesStart<'_>,
    text: &mut String,
    blocks: &mut Vec<Block>,
) -> Result<()> {
    flush(text, blocks);
    let mut id = None;
    for attribute in e.attributes() {
        let attribute = attribute?;
        if attribute.key.local_name().as_ref() == b"href" {
            let value = attribute.unescape_value()?;
            id = value
                .strip_prefix('#')
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
        }
    }
    blocks.push(Block::image(
        id.context("FB2 image must reference an embedded binary")?,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unnumbered_nested_and_repeated_content_in_reading_order() {
        let xml = r##"<FictionBook xmlns:l="http://www.w3.org/1999/xlink"><body>
        <section><title><p>Prologue</p></title><p>Before <emphasis>the</emphasis> picture.</p>
        <image l:href="#picture"/><section><title><p>Nested</p></title>
        <p><![CDATA[Inner & text]]></p><image l:href="#picture"/></section><p>After.</p></section>
        <section><image l:href="#picture"/></section></body>
        <binary id="picture">aGVsbG8=</binary></FictionBook>"##;
        let parsed = parse(xml).unwrap();
        assert_eq!(parsed.chapters.len(), 2);
        assert_eq!(parsed.chapters[0].title, "Prologue");
        assert_eq!(
            parsed.blocks[0].blocks,
            vec![
                Block::text("Before the picture."),
                Block::image("picture"),
                Block::text("Nested"),
                Block::text("Inner & text"),
                Block::image("picture"),
                Block::text("After.")
            ]
        );
        assert_eq!(parsed.blocks[1].kind, ChapterKind::Image);
        assert_eq!(parsed.images, vec![("picture".into(), b"hello".to_vec())]);
        assert!(parse(&xml.replace("id=\"picture\"", "id=\"missing\"")).is_err());
        assert!(parse(&xml.replace("#picture", "https://example.com/image.png")).is_err());
    }
}
