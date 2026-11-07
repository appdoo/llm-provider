//! OpenAiChatApi — any OpenAI-compatible `/chat/completions` server
//! (OpenAI, Ollama, LM Studio, vLLM, OpenRouter) behind `LlmProvider`.

mod projection;
mod sse;

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tokio_stream::{Stream, StreamExt};

use crate::provider::StreamEvent;

use crate::provider::anthropic::{build_client, idle_guard, read_capped, ERROR_BODY_CAP};
use crate::provider::error::{ApiError, Error};
use crate::provider::traits::*;

/// Provider for OpenAI-compatible chat/completions endpoints.
pub struct OpenAiChatApi {
    client: Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    /// Longest silence before headers or between body chunks.
    idle_gap: Duration,
}

impl OpenAiChatApi {
    /// `base_url` is the API root (e.g. `http://localhost:11434/v1`); a
    /// trailing `/` is ignored. Without `api_key` no auth header is sent.
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        let options = ProviderOptions::default();
        let client = build_client(&options).expect("HTTP client");
        Self::from_parts(client, base_url, model, api_key, options)
    }

    /// Like `new`, with explicit timeouts.
    pub fn with_options(
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        options: ProviderOptions,
    ) -> Result<Self, Error> {
        let client = build_client(&options)?;
        Ok(Self::from_parts(client, base_url, model, api_key, options))
    }

    fn from_parts(
        client: Client,
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        options: ProviderOptions,
    ) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
            idle_gap: options.idle_gap,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

/// Hand-written so the API key never reaches logs.
impl fmt::Debug for OpenAiChatApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatApi")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
}

#[async_trait]
impl LlmProvider for OpenAiChatApi {
    fn stream(
        &self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
        let body = projection::project_request(&request, &self.model);
        let mut req = self
            .client
            .post(self.endpoint())
            .header("content-type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let request_future = req.json(&body).send();
        let gap = self.idle_gap;

        Box::pin(async_stream::stream! {
            let response: reqwest::Response = match tokio::time::timeout(gap, request_future).await {
                Ok(Ok(resp)) => resp,
                Ok(Err(e)) => { yield Err(Error::Http(e)); return; }
                Err(_) => { yield Err(Error::IdleTimeout { after: gap }); return; }
            };

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let headers = response.headers().clone();
                match read_capped(response, ERROR_BODY_CAP, gap).await {
                    Ok(body) => yield Err(Error::Api(ApiError::from_response(status, &headers, &body))),
                    Err(e) => yield Err(e),
                }
                return;
            }

            let byte_stream = idle_guard(
                response.bytes_stream().map(|r| r.map_err(Error::Http)),
                gap,
            );
            let mut events = Box::pin(sse::decode_chat_stream(byte_stream));
            while let Some(ev) = events.next().await {
                yield ev;
            }
        })
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "openai-compatible".to_string(),
            model: self.model.clone(),
            supports_tools: true,
            supports_streaming: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::test_stub::StubServer;
    use crate::provider::StopReason;
    use std::sync::Arc;

    const TEXT_SSE: &str = include_str!("../../../tests/fixtures/openai_text.sse");

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

    /// `complete()` against a stub: text folds to "Hello", and no
    /// auth header is sent without a key.
    #[tokio::test]
    async fn complete_against_stub_without_key() {
        let stub = StubServer::start(vec![(
            ("POST", "/chat/completions"),
            (200, "text/event-stream", TEXT_SSE.to_string()),
        )])
        .await;
        let provider = OpenAiChatApi::new(&stub.url(), "llama3.2", None);

        let completion = provider.complete(say("Hi")).await.expect("complete");
        assert_eq!(completion.text, "Hello");
        assert_eq!(completion.stop, StopReason::EndTurn);
        assert_eq!(completion.usage.input_tokens, 9);
        assert_eq!(completion.usage.output_tokens, 2);

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert!(
            !requests[0].headers.contains_key("authorization"),
            "no auth header expected: {:?}",
            requests[0].headers
        );
        let body: serde_json::Value = serde_json::from_str(&requests[0].body).expect("json body");
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["stream"], true);
    }

    /// With a key, `Authorization: Bearer <key>` is sent; a trailing `/`
    /// on `base_url` is trimmed.
    #[tokio::test]
    async fn bearer_header_with_key_and_trailing_slash() {
        let stub = StubServer::start(vec![(
            ("POST", "/chat/completions"),
            (200, "text/event-stream", TEXT_SSE.to_string()),
        )])
        .await;
        let provider = OpenAiChatApi::new(
            &format!("{}/", stub.url()),
            "gpt-4o-mini",
            Some("sk-abc".to_string()),
        );

        provider.complete(say("Hi")).await.expect("complete");
        let requests = stub.requests();
        assert_eq!(requests[0].path, "/chat/completions");
        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer sk-abc")
        );
    }

    /// A non-2xx response is one `Api` error carrying status and body.
    #[tokio::test]
    async fn non_2xx_is_api_error_with_body() {
        let stub = StubServer::start(vec![(
            ("POST", "/chat/completions"),
            (
                401,
                "application/json",
                r#"{"error":"bad key"}"#.to_string(),
            ),
        )])
        .await;
        let provider = OpenAiChatApi::new(&stub.url(), "m", None);

        match provider.complete(say("Hi")).await {
            Err(Error::Api(e)) => {
                assert_eq!(e.status, Some(401));
                assert_eq!(e.message, r#"{"error":"bad key"}"#);
                assert_eq!(e.kind, crate::provider::error::ErrorKind::Auth);
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    /// The key is masked.
    #[test]
    fn debug_masks_api_key() {
        let provider = OpenAiChatApi::new(
            "http://localhost:11434/v1",
            "llama3.2",
            Some("sk-test-SECRET".into()),
        );
        let rendered = format!("{provider:?}");
        assert_eq!(
            rendered,
            r#"OpenAiChatApi { base_url: "http://localhost:11434/v1", model: "llama3.2", api_key: Some("***") }"#
        );
        assert!(!rendered.contains("sk-test-SECRET"));

        let keyless = OpenAiChatApi::new("http://localhost:11434/v1", "llama3.2", None);
        assert!(format!("{keyless:?}").contains("api_key: None"));
    }

    #[test]
    fn info_reports_openai_compatible() {
        let info = OpenAiChatApi::new("http://x/v1", "llama3.2", None).info();
        assert_eq!(info.name, "openai-compatible");
        assert_eq!(info.model, "llama3.2");
        assert!(info.supports_tools);
        assert!(info.supports_streaming);
    }
}
