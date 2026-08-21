//! Translating chunks via the DeepSeek API and building prompts.

pub mod deepseek;
pub mod prompt;
pub mod reply;

pub use deepseek::DeepSeekClient;

use anyhow::Result;

use crate::config::Config;
use crate::glossary::{self, Term};

/// Extract glossary terms from a source ↔ translation pair, retrying when the
/// model's reply is not valid JSON.
///
/// Two failure classes are handled at different layers:
/// - **network / HTTP** (DeepSeek down, 429, 5xx, timeout) is already retried
///   with backoff inside [`DeepSeekClient::translate`], and bubbles up here;
/// - **a successful response whose term list is malformed JSON** is retried here
///   up to `retries` times (asking the model again usually fixes it).
pub async fn extract_terms(
    client: &DeepSeekClient,
    config: &Config,
    source: &str,
    translated: &str,
    retries: usize,
) -> Result<Vec<Term>> {
    let (system, user) = glossary::build_extraction_prompt(config, source, translated);
    let mut last_err = None;
    for attempt in 0..=retries {
        // Bounded reply, so it can safely ask for strict JSON.
        let raw = client.translate_json(&system, &user).await?;
        match glossary::parse_extracted_terms(&raw) {
            Ok(terms) => return Ok(terms),
            Err(e) => {
                tracing::warn!(attempt, "extraction JSON parse failed, retrying: {e:#}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("term extraction failed")))
}
