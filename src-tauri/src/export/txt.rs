//! Export to a single .txt: chapters in order, each with its title.

use std::path::Path;

use anyhow::{Context, Result};

use super::TranslatedChapter;

/// Render the book as a single plain-text string.
pub fn render(chapters: &[TranslatedChapter]) -> String {
    let mut out = String::new();
    for ch in chapters {
        let title = ch.title.trim();
        if !title.is_empty() {
            out.push_str(title);
            out.push_str("\n\n");
        }
        out.push_str(ch.body.trim());
        out.push_str("\n\n\n");
    }
    out
}

/// Assemble the book into a single .txt file.
pub fn export(chapters: &[TranslatedChapter], out_path: &Path) -> Result<()> {
    std::fs::write(out_path, render(chapters))
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_chapters_in_order() {
        let chapters = vec![
            TranslatedChapter { index: 1, number: Some(1), title: "Глава 1".into(), body: "Один.".into() },
            TranslatedChapter { index: 2, number: Some(2), title: "Глава 2".into(), body: "Два.".into() },
        ];
        let path = std::env::temp_dir().join(format!("bc_txt_{}.txt", std::process::id()));
        export(&chapters, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(text.contains("Глава 1"));
        assert!(text.contains("Один."));
        // order preserved
        assert!(text.find("Глава 1").unwrap() < text.find("Глава 2").unwrap());
    }
}
