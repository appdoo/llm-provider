//! Level 0: LLM provider abstraction — Anthropic and OpenAI-compatible
//! providers, batch, retry, and a factory from `ProviderConfig`.

mod anthropic;
mod auth;
mod batch;
mod error;
mod event;
mod openai;
mod request;
mod retry;
mod sse;
#[cfg(test)]
mod test_stub;
mod traits;

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use anthropic::Anthropic;
use anthropic::AnthropicApi;
pub use auth::AuthMethod;
pub use batch::{
    BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState, BatchStatus,
};
pub use error::{ApiError, Error, ErrorKind};
pub use event::{StopReason, StreamEvent};
use openai::OpenAiChatApi;
pub use openai::OpenAiCompatible;
pub use request::Request;
pub use retry::RetryPolicy;
pub use traits::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, Completion, CompletionRequest, Content,
    LlmProvider, Message, MessageRole, ModelSelection, ProviderInfo, ProviderOptions,
    ToolDefinition, ToolUseCall, Usage,
};

/// Provider selection and resilience settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub default: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    /// Total attempts per request; 1 turns retry off. Default 4.
    pub max_attempts: Option<u32>,
    /// TCP/TLS connect limit in seconds. Default 30.
    pub connect_timeout_secs: Option<u64>,
    /// Longest silence on a response in seconds. Default 90.
    pub stream_idle_timeout_secs: Option<u64>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            default: "claude".to_string(),
            model: None,
            base_url: None,
            max_attempts: None,
            connect_timeout_secs: None,
            stream_idle_timeout_secs: None,
        }
    }
}

/// Timeouts and retry policy from configuration; unset fields take the
/// defaults (4 attempts, connect 30 s, idle 90 s). `base` and `cap` are fixed.
pub fn resilience_from_config(c: &ProviderConfig) -> (ProviderOptions, RetryPolicy) {
    let options = ProviderOptions {
        connect_timeout: Duration::from_secs(c.connect_timeout_secs.unwrap_or(30)),
        idle_gap: Duration::from_secs(c.stream_idle_timeout_secs.unwrap_or(90)),
    };
    let policy = RetryPolicy {
        max_attempts: c.max_attempts.unwrap_or(4),
        ..RetryPolicy::default()
    };
    (options, policy)
}

/// Create a provider from configuration, with retry on (`RetryPolicy` from
/// the config).
pub fn create_provider(
    config: &ProviderConfig,
    auth: Option<AuthMethod>,
) -> Result<Box<dyn LlmProvider>, Error> {
    match config.default.as_str() {
        "claude" | "anthropic" => {
            let auth = auth.ok_or_else(|| Error::MissingCredentials {
                message: "anthropic provider needs a credential".to_string(),
            })?;
            Ok(Box::new(create_anthropic(config, auth)?))
        }
        "openai" => {
            let (options, policy) = resilience_from_config(config);
            let api_key = auth.map(|a| match a {
                AuthMethod::ApiKey(k) | AuthMethod::Bearer(k) => k,
            });
            let provider = create_openai(config, options, api_key)?;
            Ok(Box::new(OpenAiCompatible::from_transport(provider, policy)))
        }
        other => Err(Error::UnknownProvider(other.to_string())),
    }
}

/// Build an Anthropic provider from configuration, with retry on
/// (`RetryPolicy` from the config). Keeps `BatchProvider`. Does not read `config.default`.
pub fn create_anthropic(config: &ProviderConfig, auth: AuthMethod) -> Result<Anthropic, Error> {
    let (options, policy) = resilience_from_config(config);
    let model = config.model.as_deref().unwrap_or("claude-sonnet-4-6");
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or(anthropic::DEFAULT_BASE_URL);
    let provider = AnthropicApi::with_options(model, auth, base_url, options)?;
    Ok(Anthropic::from_transport(provider, policy))
}

/// Build an `OpenAiChatApi`. `base_url` and `model` are mandatory so
/// a local setup never falls through to a hosted API by accident.
fn create_openai(
    config: &ProviderConfig,
    options: ProviderOptions,
    api_key: Option<String>,
) -> Result<OpenAiChatApi, Error> {
    let base_url = config
        .base_url
        .as_deref()
        .ok_or_else(|| Error::InvalidRequest("openai provider needs base_url".to_string()))?;
    let model = config
        .model
        .as_deref()
        .ok_or_else(|| Error::InvalidRequest("openai provider needs model".to_string()))?;
    OpenAiChatApi::with_options(base_url, model, api_key, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::test_stub::StubServer;
    use std::sync::Arc;

    const TEXT_SSE: &str = include_str!("../../tests/fixtures/openai_text.sse");
    const ANTHROPIC_SSE: &str = include_str!("../../tests/fixtures/anthropic_text.sse");

    fn say(text: &str) -> CompletionRequest {
        CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: text.to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 64,
            cache_breakpoints: Vec::new(),
        }
    }

    async fn openai_stub() -> StubServer {
        StubServer::start(vec![(
            ("POST", "/chat/completions"),
            (200, "text/event-stream", TEXT_SSE.to_string()),
        )])
        .await
    }

    fn openai_config(base_url: String) -> ProviderConfig {
        ProviderConfig {
            default: "openai".into(),
            model: Some("m".into()),
            base_url: Some(base_url),
            ..Default::default()
        }
    }

    /// An explicit credential builds the Anthropic provider.
    #[tokio::test]
    async fn create_provider_anthropic_with_explicit_credential() {
        let config = ProviderConfig {
            default: "anthropic".into(),
            ..Default::default()
        };
        let provider = create_provider(&config, Some(AuthMethod::ApiKey("k".into())))
            .expect("explicit credential builds the provider");
        assert_eq!(provider.info().name, "anthropic");
    }

    /// The factory builds an OpenAI-compatible provider without a key;
    /// no `authorization` header is sent.
    #[tokio::test]
    async fn create_provider_openai_without_key_sends_no_auth_header() {
        let stub = openai_stub().await;
        let provider =
            create_provider(&openai_config(stub.url()), None).expect("factory builds provider");

        provider.complete(say("Hi")).await.expect("complete");

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert!(
            !requests[0].headers.contains_key("authorization"),
            "no auth header expected: {:?}",
            requests[0].headers
        );
    }

    /// The key passed to the factory reaches the request header.
    #[tokio::test]
    async fn create_provider_openai_key_travels_through_factory() {
        let stub = openai_stub().await;
        let provider = create_provider(
            &openai_config(stub.url()),
            Some(AuthMethod::ApiKey("sk-x".into())),
        )
        .expect("factory builds provider");

        provider.complete(say("Hi")).await.expect("complete");

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer sk-x")
        );
    }

    /// `create_provider` sends Anthropic traffic to `config.base_url`.
    #[tokio::test]
    async fn create_provider_anthropic_honours_base_url() {
        let stub = StubServer::start(vec![(
            ("POST", "/v1/messages"),
            (200, "text/event-stream", ANTHROPIC_SSE.to_string()),
        )])
        .await;
        let config = ProviderConfig {
            default: "anthropic".into(),
            base_url: Some(stub.url()),
            ..Default::default()
        };
        let provider = create_provider(&config, Some(AuthMethod::ApiKey("sk-test".into())))
            .expect("factory builds provider");

        provider.complete(say("Hi")).await.expect("complete");

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/messages");
        assert_eq!(
            requests[0].headers.get("x-api-key").map(String::as_str),
            Some("sk-test")
        );
    }

    /// `create_anthropic` keeps `BatchProvider` and honours `base_url`.
    #[tokio::test]
    async fn create_anthropic_keeps_batches() {
        let stub = StubServer::start(vec![(
            ("POST", "/v1/messages/batches"),
            (
                200,
                "application/json",
                r#"{"id":"msgbatch_1","type":"message_batch","processing_status":"in_progress"}"#
                    .to_string(),
            ),
        )])
        .await;
        let config = ProviderConfig {
            base_url: Some(stub.url()),
            ..Default::default()
        };
        let provider = create_anthropic(&config, AuthMethod::ApiKey("sk-test".into()))
            .expect("factory builds provider");

        let id = provider
            .submit(vec![BatchRequest {
                custom_id: "yt_abc".into(),
                request: say("Hi"),
            }])
            .await
            .expect("submit");

        assert_eq!(id, BatchId("msgbatch_1".into()));
        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/messages/batches");
    }
}
