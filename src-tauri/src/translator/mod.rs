//! Translating chunks via the DeepSeek API and building prompts.

pub mod deepseek;
pub mod prompt;
pub mod repair;
pub mod reply;

pub use deepseek::DeepSeekClient;

use std::future::Future;

use anyhow::Result;

use crate::config::Config;
use crate::glossary::{self, Term};

/// What the orchestration layer needs from a model.
///
/// The orchestrator is generic over this rather than tied to
/// [`DeepSeekClient`], so the translation loop can be driven end to end in
/// tests against a scripted stand-in: chapter order, resume, the language
/// repair pass, glossary growth. None of that was reachable before, because
/// exercising it meant calling the real API.
///
/// Retry, backoff and truncation handling stay inside the implementation; this
/// trait is only "send a prompt, get a reply".
pub trait Translate {
    /// Prose reply. May be continued internally if the model truncates it.
    fn translate(&self, system: &str, user: &str) -> impl Future<Output = Result<String>> + Send;

    /// One JSON document. Must not be a reply that can outgrow the output limit.
    fn translate_json(
        &self,
        system: &str,
        user: &str,
    ) -> impl Future<Output = Result<String>> + Send;
}

impl Translate for DeepSeekClient {
    fn translate(&self, system: &str, user: &str) -> impl Future<Output = Result<String>> + Send {
        DeepSeekClient::translate(self, system, user)
    }

    fn translate_json(
        &self,
        system: &str,
        user: &str,
    ) -> impl Future<Output = Result<String>> + Send {
        DeepSeekClient::translate_json(self, system, user)
    }
}

/// Extract glossary terms from a source ↔ translation pair, retrying when the
/// model's reply is not valid JSON.
///
/// Two failure classes are handled at different layers:
/// - **network / HTTP** (DeepSeek down, 429, 5xx, timeout) is already retried
///   with backoff inside [`DeepSeekClient::translate`], and bubbles up here;
/// - **a successful response whose term list is malformed JSON** is retried here
///   up to `retries` times (asking the model again usually fixes it).
pub async fn extract_terms<C: Translate>(
    client: &C,
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
