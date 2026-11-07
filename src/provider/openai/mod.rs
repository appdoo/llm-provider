//! OpenAiChatApi — any OpenAI-compatible `/chat/completions` server
//! (OpenAI, Ollama, LM Studio, vLLM, OpenRouter) behind `LlmProvider`.

// Not yet restructured to the rust-design standard; pedantic applies once it is.
#![allow(clippy::pedantic)]

mod projection;
mod sse;

use std::fmt;
use std::pin::Pin;

use async_trait::async_trait;
use tokio_stream::{Stream, StreamExt};

use crate::provider::StreamEvent;

use crate::provider::error::Error;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::provider::traits::*;
use crate::provider::Request;
use crate::transport::Http;

/// Provider for OpenAI-compatible chat/completions endpoints.
#[derive(Clone)]
pub(crate) struct OpenAiChatApi {
    http: Http,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiChatApi {
    /// `base_url` is the API root (e.g. `http://localhost:11434/v1`); a
    /// trailing `/` is ignored. Without `api_key` no auth header is sent.
    pub(crate) fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self::with_options(base_url, model, api_key, ProviderOptions::default())
    }

    /// Like `new`, with explicit timeouts.
    pub(crate) fn with_options(
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        options: ProviderOptions,
    ) -> Self {
        Self {
            http: Http::new(options),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

/// Client for an OpenAI-compatible server with retry on by default.
#[derive(Clone)]
pub struct OpenAiCompatible {
    inner: RetryingProvider<OpenAiChatApi>,
}

impl OpenAiCompatible {
    pub(crate) fn from_transport(api: OpenAiChatApi, policy: RetryPolicy) -> Self {
        Self {
            inner: RetryingProvider::new(api, policy),
        }
    }

    /// A client for `model` at `base_url` (the API root, e.g.
    /// `http://localhost:11434/v1`). It takes the base URL because a local
    /// server has no default address, while `Anthropic::new` takes the
    /// credential because its server has one. No key is sent unless
    /// `api_key` is set.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::from_transport(
            OpenAiChatApi::new(&base_url.into(), &model.into(), None),
            RetryPolicy::default(),
        )
    }

    /// Send `Authorization: Bearer <key>`.
    pub fn api_key(self, key: impl Into<String>) -> Self {
        let mut api = self.inner.inner().clone();
        api.api_key = Some(key.into());
        Self::from_transport(api, self.inner.policy())
    }

    /// Replace the connect and idle timeouts.
    pub fn timeouts(self, options: ProviderOptions) -> Self {
        let mut api = self.inner.inner().clone();
        api.http = Http::new(options);
        Self::from_transport(api, self.inner.policy())
    }

    /// Replace the retry policy; `RetryPolicy::none()` turns retry off.
    pub fn retry(self, policy: RetryPolicy) -> Self {
        Self::from_transport(self.inner.inner().clone(), policy)
    }

    /// Ask one question and return the answer text.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        (self as &dyn LlmProvider).ask(prompt).await
    }

    /// Start a request with optional system prompt, model and token limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        (self as &dyn LlmProvider).request(prompt)
    }
}

/// Hand-written so the API key never reaches logs.
impl fmt::Debug for OpenAiCompatible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api = self.inner.inner();
        f.debug_struct("OpenAiCompatible")
            .field("model", &api.model)
            .field("base_url", &api.base_url)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    fn stream(
        &self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
        self.inner.stream(request)
    }

    fn info(&self) -> ProviderInfo {
        self.inner.info()
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
        let api = self.clone();
        Box::pin(async_stream::try_stream! {
            let body = projection::project_request(&request, &api.model);
            let mut req = api
                .http
                .client()
                .await?
                .post(api.endpoint())
                .header("content-type", "application/json");
            if let Some(key) = &api.api_key {
                req = req.bearer_auth(key);
            }

            let bytes = api.http.byte_stream(req.json(&body)).await?;
            let mut events = Box::pin(sse::decode_chat_stream(bytes));
            while let Some(event) = events.next().await {
                yield event?;
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
                assert_eq!(e.status(), Some(401));
                assert_eq!(e.message(), r#"{"error":"bad key"}"#);
                assert_eq!(e.kind(), crate::provider::error::ErrorKind::Auth);
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

    /// A3: `ask` works without a key; `api_key` adds the Bearer header.
    #[tokio::test]
    async fn front_ask_with_and_without_key() {
        let stub = StubServer::start(vec![(
            ("POST", "/chat/completions"),
            (200, "text/event-stream", TEXT_SSE.to_string()),
        )])
        .await;
        let local = OpenAiCompatible::new(stub.url(), "qwen3");
        assert_eq!(local.ask("Hello").await.expect("ask"), "Hello");
        assert_eq!(
            local
                .clone()
                .api_key("k")
                .ask("Hello")
                .await
                .expect("ask with key"),
            "Hello"
        );
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0].headers.contains_key("authorization"));
        assert_eq!(
            requests[1].headers.get("authorization").map(String::as_str),
            Some("Bearer k")
        );
        for r in &requests {
            let body: serde_json::Value = serde_json::from_str(&r.body).expect("json");
            assert_eq!(body["model"], "qwen3");
        }
    }
}
