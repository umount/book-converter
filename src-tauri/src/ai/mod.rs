//! Shared provider transport. Domain prompts and persistence stay outside this module.
use crate::app::contracts::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderProfile {
    pub id: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
    pub timeout_seconds: u64,
    pub network_retries: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}
impl Message {
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(Content::Text(text.into())),
            tool_call_id: None,
            tool_calls: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
#[derive(Debug, Clone)]
pub enum Request {
    Text {
        system: String,
        user: String,
    },
    Structured {
        system: String,
        user: String,
    },
    Vision {
        system: String,
        parts: Vec<ContentPart>,
    },
    ToolConversation {
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    },
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}
#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    pub finish_reason: String,
    pub usage: Usage,
    pub tool_calls: Vec<ToolCall>,
}

pub trait Provider: Send + Sync {
    fn profile(&self) -> &ProviderProfile;
    fn complete(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>>;
}

pub struct ChatCompletions {
    profile: ProviderProfile,
    http: reqwest::Client,
    credential: String,
}
impl ChatCompletions {
    pub fn new(profile: ProviderProfile, credential: String) -> Result<Self, AppError> {
        let url =
            reqwest::Url::parse(&profile.base_url).map_err(|_| AppError::invalid("providerUrl"))?;
        if !["http", "https"].contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AppError::invalid("providerUrl"));
        }
        if credential.trim().is_empty() {
            return Err(error(
                ErrorCode::CapabilityUnavailable,
                "errors.apiKeyRequired",
                false,
            ));
        }
        if profile.model.is_empty()
            || profile.max_output_tokens == 0
            || profile.timeout_seconds == 0
            || profile.network_retries > 5
            || !profile.temperature.is_finite()
            || !(0.0..=2.0).contains(&profile.temperature)
        {
            return Err(AppError::invalid("providerProfile"));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(profile.timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| error(ErrorCode::Provider, "errors.providerTransport", false))?;
        Ok(Self {
            profile,
            http,
            credential,
        })
    }
    async fn send(&self, request: Request) -> Result<Completion, AppError> {
        static NETWORK: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
        let _permit = NETWORK
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .acquire()
            .await
            .map_err(|_| error(ErrorCode::Provider, "errors.providerTransport", true))?;
        let body = request_body(&self.profile, request)?;
        let bytes = serde_json::to_vec(&body).map_err(|_| AppError::invalid("request"))?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(AppError::invalid("requestSize"));
        }
        for attempt in 0..=self.profile.network_retries {
            let result = self.once(&bytes).await;
            match result {
                Err(ref e) if e.retryable && attempt < self.profile.network_retries => {
                    tokio::time::sleep(Duration::from_millis((250u64 << attempt).min(8000))).await;
                }
                _ => return result,
            }
        }
        unreachable!("finite retry loop returns on its final attempt")
    }
    async fn once(&self, body: &[u8]) -> Result<Completion, AppError> {
        let mut response = self
            .http
            .post(format!(
                "{}/chat/completions",
                self.profile.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.credential)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| error(ErrorCode::Provider, "errors.providerTransport", true))?;
        let status = response.status();
        if !status.is_success() {
            let mut failure = error(
                ErrorCode::Provider,
                "errors.providerHttp",
                status.as_u16() == 429 || status.is_server_error(),
            );
            failure
                .params
                .insert("status".into(), status.as_u16().to_string());
            return Err(failure);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| error(ErrorCode::Provider, "errors.providerTransport", true))?
        {
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                return Err(error(
                    ErrorCode::InvalidOutput,
                    "errors.providerResponseSize",
                    false,
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        parse_completion(&bytes)
    }
}
impl Provider for ChatCompletions {
    fn profile(&self) -> &ProviderProfile {
        &self.profile
    }
    fn complete(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
        Box::pin(self.send(request))
    }
}
fn error(code: ErrorCode, key: &str, retryable: bool) -> AppError {
    AppError {
        code,
        message_key: key.into(),
        params: Default::default(),
        retryable,
    }
}
fn request_body(
    profile: &ProviderProfile,
    request: Request,
) -> Result<serde_json::Value, AppError> {
    let (messages, json, tools) = match request {
        Request::Text { system, user } => (
            vec![Message::text("system", system), Message::text("user", user)],
            false,
            vec![],
        ),
        Request::Structured { system, user } => (
            vec![Message::text("system", system), Message::text("user", user)],
            true,
            vec![],
        ),
        Request::Vision { system, parts } => (
            vec![
                Message::text("system", system),
                Message {
                    role: "user".into(),
                    content: Some(Content::Parts(parts)),
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            true,
            vec![],
        ),
        Request::ToolConversation { messages, tools } => (messages, false, tools),
    };
    if messages.is_empty() {
        return Err(AppError::invalid("messages"));
    }
    let mut body = serde_json::json!({"model":profile.model,"messages":messages,"temperature":profile.temperature,"max_tokens":profile.max_output_tokens,"stream":false});
    if json {
        body["response_format"] = serde_json::json!({"type":"json_object"});
    }
    if !tools.is_empty() {
        body["tools"] = tools
            .into_iter()
            .map(|function| serde_json::json!({"type":"function","function":function}))
            .collect();
    }
    Ok(body)
}
fn parse_completion(bytes: &[u8]) -> Result<Completion, AppError> {
    #[derive(Deserialize)]
    struct Response {
        choices: Vec<Choice>,
        #[serde(default)]
        usage: Usage,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Reply,
        finish_reason: String,
    }
    #[derive(Deserialize)]
    struct Reply {
        content: Option<String>,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
    }
    let parsed: Response = serde_json::from_slice(bytes)
        .map_err(|_| error(ErrorCode::InvalidOutput, "errors.providerResponse", false))?;
    if parsed.choices.len() != 1 {
        return Err(error(
            ErrorCode::InvalidOutput,
            "errors.providerChoices",
            false,
        ));
    }
    let choice = parsed.choices.into_iter().next().expect("one choice");
    Ok(Completion {
        text: choice.message.content.unwrap_or_default(),
        finish_reason: choice.finish_reason,
        usage: parsed.usage,
        tool_calls: choice.message.tool_calls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finish_reason_and_usage_survive_parsing() {
        let value=parse_completion(br#"{"choices":[{"message":{"content":"partial"},"finish_reason":"length"}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#).unwrap();
        assert_eq!(value.finish_reason, "length");
        assert_eq!(value.usage.prompt_tokens, 7);
        assert!(parse_completion(br#"{"choices":[]}"#).is_err());
    }
    #[test]
    fn structured_request_is_json_without_transport_credentials() {
        let profile = ProviderProfile {
            id: "book".into(),
            base_url: "https://example.test".into(),
            model: "explicit-model".into(),
            temperature: 0.3,
            max_output_tokens: 4000,
            timeout_seconds: 60,
            network_retries: 2,
        };
        let body = request_body(
            &profile,
            Request::Structured {
                system: "Return JSON".into(),
                user: "Text".into(),
            },
        )
        .unwrap();
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["model"], "explicit-model");
        assert!(body.get("api_key").is_none());
    }
}
