//! Клиент DeepSeek API (OpenAI-совместимый `/chat/completions`).
//!
//! Отвечает за один запрос перевода: retry с экспоненциальным backoff,
//! обработку 429/5xx, таймауты. Параллелизм и очередь — на уровне `state`/UI.

use crate::config::Config;

pub struct DeepSeekClient {
    http: reqwest::Client,
    config: Config,
}

impl DeepSeekClient {
    pub fn new(config: Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    /// Перевести один готовый промпт (system + user) и вернуть текст.
    ///
    /// TODO:
    /// - POST {base_url}/chat/completions с моделью config.model;
    /// - заголовок Authorization: Bearer {api_key};
    /// - retry с backoff при 429/5xx/сетевых ошибках (до max_retries);
    /// - вернуть content первого choice.
    pub async fn translate(&self, _system: &str, _user: &str) -> anyhow::Result<String> {
        let _ = (&self.http, &self.config);
        todo!("запрос к DeepSeek /chat/completions с retry")
    }
}
