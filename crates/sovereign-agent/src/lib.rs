use std::net::Ipv6Addr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::{Host, Url};

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
    pub allow_remote: bool,
    pub timeout: Duration,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct ChatCompletionResponse {
    pub text: String,
    pub model: Option<String>,
    pub network_destination: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointScope {
    Loopback,
    PrivateNetwork,
    PublicInternet,
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
        validate_endpoint_access(&endpoint, request.allow_remote)?;

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

pub fn classify_endpoint(endpoint: &Url) -> Result<EndpointScope> {
    match endpoint.scheme() {
        "http" | "https" => {}
        scheme => bail!("unsupported endpoint scheme '{scheme}'"),
    }

    let Some(host) = endpoint.host() else {
        bail!("endpoint must include a host");
    };

    match host {
        Host::Domain(domain) if domain.eq_ignore_ascii_case("localhost") => {
            Ok(EndpointScope::Loopback)
        }
        Host::Domain(_) => Ok(EndpointScope::PublicInternet),
        Host::Ipv4(ipv4) if ipv4.is_loopback() => Ok(EndpointScope::Loopback),
        Host::Ipv4(ipv4) if ipv4.is_private() || ipv4.is_link_local() => {
            Ok(EndpointScope::PrivateNetwork)
        }
        Host::Ipv4(_) => Ok(EndpointScope::PublicInternet),
        Host::Ipv6(ipv6) if ipv6.is_loopback() => Ok(EndpointScope::Loopback),
        Host::Ipv6(ipv6) if is_private_ipv6(ipv6) => Ok(EndpointScope::PrivateNetwork),
        Host::Ipv6(_) => Ok(EndpointScope::PublicInternet),
    }
}

pub fn classify_endpoint_url(endpoint: &str) -> Result<EndpointScope> {
    let endpoint =
        Url::parse(endpoint).with_context(|| format!("invalid endpoint URL '{endpoint}'"))?;
    classify_endpoint(&endpoint)
}

fn validate_endpoint_access(endpoint: &Url, allow_remote: bool) -> Result<()> {
    let scope = classify_endpoint(endpoint)?;
    if scope == EndpointScope::PublicInternet && !allow_remote {
        bail!(
            "remote endpoint '{}' requires allow_remote = true",
            endpoint.host_str().unwrap_or("<missing host>")
        );
    }
    Ok(())
}

fn is_private_ipv6(address: Ipv6Addr) -> bool {
    let first_segment = address.segments()[0];
    let is_unique_local = (first_segment & 0xfe00) == 0xfc00;
    let is_link_local = (first_segment & 0xffc0) == 0xfe80;
    is_unique_local || is_link_local
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_loopback_endpoints_as_local_machine() {
        let endpoint = Url::parse("http://127.0.0.1:8000/v1/chat/completions").expect("url");
        assert_eq!(
            classify_endpoint(&endpoint).expect("scope"),
            EndpointScope::Loopback
        );

        let endpoint = Url::parse("http://localhost:8000/v1/chat/completions").expect("url");
        assert_eq!(
            classify_endpoint(&endpoint).expect("scope"),
            EndpointScope::Loopback
        );

        let endpoint = Url::parse("http://[::1]:8000/v1/chat/completions").expect("url");
        assert_eq!(
            classify_endpoint(&endpoint).expect("scope"),
            EndpointScope::Loopback
        );
    }

    #[test]
    fn classifies_private_network_endpoints() {
        let endpoint = Url::parse("http://192.168.1.10:8000/v1/chat/completions").expect("url");
        assert_eq!(
            classify_endpoint(&endpoint).expect("scope"),
            EndpointScope::PrivateNetwork
        );

        let endpoint = Url::parse("http://[fd00::1]:8000/v1/chat/completions").expect("url");
        assert_eq!(
            classify_endpoint(&endpoint).expect("scope"),
            EndpointScope::PrivateNetwork
        );
    }

    #[test]
    fn blocks_public_endpoints_without_explicit_remote_allowance() {
        let endpoint = Url::parse("https://api.openai.com/v1/chat/completions").expect("url");
        let error = validate_endpoint_access(&endpoint, false).expect_err("blocked");

        assert!(error.to_string().contains("allow_remote = true"));
    }

    #[test]
    fn allows_public_endpoints_when_remote_is_explicit() {
        let endpoint = Url::parse("https://api.openai.com/v1/chat/completions").expect("url");
        validate_endpoint_access(&endpoint, true).expect("allowed");
    }

    #[test]
    fn rejects_unsupported_endpoint_schemes() {
        let endpoint = Url::parse("file:///tmp/model.sock").expect("url");
        let error = validate_endpoint_access(&endpoint, true).expect_err("unsupported");

        assert!(error.to_string().contains("unsupported endpoint scheme"));
    }
}
