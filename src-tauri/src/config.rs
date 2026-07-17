//! Конфигурация приложения: доступ к DeepSeek API и параметры перевода.
//!
//! Загружается из `config.local.toml` или переменных окружения.
//! Секреты (API-ключ) в репозиторий не коммитятся (см. `.gitignore`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Ключ DeepSeek API (env: DEEPSEEK_API_KEY).
    pub api_key: String,
    /// Базовый URL API (OpenAI-совместимый).
    pub base_url: String,
    /// Модель: "deepseek-chat" или "deepseek-reasoner".
    pub model: String,

    /// Язык оригинала, напр. "китайский".
    pub source_lang: String,
    /// Язык перевода, напр. "русский".
    pub target_lang: String,

    /// Сколько глав переводить параллельно.
    pub concurrency: usize,
    /// Максимальный размер чанка в символах (fallback-разбиение длинных глав).
    pub max_chunk_chars: usize,
    /// Число повторов при сетевых ошибках / 429.
    pub max_retries: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            source_lang: "китайский".into(),
            target_lang: "русский".into(),
            concurrency: 4,
            max_chunk_chars: 6000,
            max_retries: 5,
        }
    }
}

impl Config {
    /// TODO: загрузить из файла/окружения, ключ — из env DEEPSEEK_API_KEY.
    pub fn load() -> anyhow::Result<Self> {
        todo!("загрузка конфигурации из config.local.toml + env")
    }
}
