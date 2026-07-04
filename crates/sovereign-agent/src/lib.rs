use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatCompletionRequest {
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct ChatCompletionResponse {
    pub text: String,
    pub model: Option<String>,
    pub network_destination: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    model: Option<String>,
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

pub struct OpenAiCompatibleClient {
    client: Client,
}

impl OpenAiCompatibleClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn chat(&self, request: ChatCompletionRequest) -> Result<ChatCompletionResponse> {
        let endpoint = Url::parse(&request.endpoint)
            .with_context(|| format!("invalid endpoint URL '{}'", request.endpoint))?;
        validate_local_first_endpoint(&endpoint)?;

        let payload = json!({
            "model": request.model,
            "messages": request.messages,
            "stream": false,
        });

        let mut builder = self
            .client
            .post(endpoint.clone())
            .timeout(request.timeout)
            .json(&payload);

        if let Some(api_key) = request.api_key {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder
            .send()
            .await
            .with_context(|| format!("failed to reach {}", endpoint))?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .context("failed to read response body")?;

        if !status.is_success() {
            bail!("provider returned HTTP {status}: {raw}");
        }

        let parsed: OpenAiChatResponse =
            serde_json::from_str(&raw).context("provider did not return OpenAI chat JSON")?;
        let text = parsed
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .unwrap_or_default();

        Ok(ChatCompletionResponse {
            text,
            model: parsed.model,
            network_destination: endpoint.to_string(),
        })
    }
}

impl Default for OpenAiCompatibleClient {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_local_first_endpoint(endpoint: &Url) -> Result<()> {
    match endpoint.scheme() {
        "http" | "https" => Ok(()),
        scheme => bail!("unsupported endpoint scheme '{scheme}'"),
    }
}
