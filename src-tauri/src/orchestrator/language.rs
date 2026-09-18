//! Target-language validation and line-scoped repair stage.

use crate::config::Config;
use crate::textutil::{self, MIN_FOREIGN_RUN};
use crate::translator::{repair, Translate};

const MAX_LANGUAGE_REPAIRS: usize = 2;

pub(super) struct LanguageRepairer<'a, C: Translate> {
    client: &'a C,
    config: &'a Config,
}

impl<'a, C: Translate> LanguageRepairer<'a, C> {
    pub(super) fn new(client: &'a C, config: &'a Config) -> Self {
        Self { client, config }
    }

    pub(super) async fn enforce(
        &self,
        chapter_index: usize,
        title: &str,
        body: &str,
        source: &str,
    ) -> (String, String, Vec<String>) {
        let Some(expected) = textutil::expected_script(&self.config.target_lang) else {
            return (title.to_string(), body.to_string(), Vec::new());
        };

        // Line zero is the title. Splitting on '\n' preserves blank lines.
        let mut lines: Vec<String> = std::iter::once(title.to_string())
            .chain(body.split('\n').map(str::to_string))
            .collect();
        let joined = |lines: &[String]| lines.join("\n");
        let mut fragments =
            textutil::foreign_fragments(&joined(&lines), expected, source, MIN_FOREIGN_RUN);

        for attempt in 0..MAX_LANGUAGE_REPAIRS {
            if fragments.is_empty() {
                break;
            }
            let targets = repair::lines_with_fragments(&lines, &fragments);
            if targets.is_empty() {
                break;
            }
            let numbered: Vec<repair::NumberedLine> = targets
                .iter()
                .map(|&number| repair::NumberedLine {
                    n: number,
                    text: lines[number].clone(),
                })
                .collect();
            let sent_chars: usize = numbered.iter().map(|line| line.text.chars().count()).sum();
            tracing::info!(
                chapter = chapter_index,
                attempt,
                fragments = ?fragments,
                lines = numbered.len(),
                of_lines = lines.len(),
                chars = sent_chars,
                "translation kept foreign words; repairing the affected lines"
            );

            let mut changed = 0usize;
            for batch in repair::batches(&numbered, repair::MAX_BATCH_CHARS) {
                let (system, user) = repair::build_prompt(self.config, &fragments, &batch);
                let raw = match self.client.translate_json(&system, &user).await {
                    Ok(raw) => raw,
                    Err(error) => {
                        tracing::warn!(
                            chapter = chapter_index,
                            "language repair request failed: {error:#}"
                        );
                        break;
                    }
                };
                match repair::parse_reply(&raw) {
                    Ok(fixed) => changed += repair::splice(&mut lines, &fixed),
                    Err(error) => tracing::warn!(
                        chapter = chapter_index,
                        "unparseable repair reply: {error:#}"
                    ),
                }
            }
            if changed == 0 {
                break;
            }

            let remaining =
                textutil::foreign_fragments(&joined(&lines), expected, source, MIN_FOREIGN_RUN);
            let progressed = remaining.len() < fragments.len();
            fragments = remaining;
            if !progressed {
                break;
            }
        }

        if !fragments.is_empty() {
            tracing::warn!(
                chapter = chapter_index,
                fragments = ?fragments,
                "foreign words remain after repair"
            );
        }

        let title = lines.remove(0);
        (title, lines.join("\n").trim().to_string(), fragments)
    }
}
