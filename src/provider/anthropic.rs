//! AnthropicApi — direct HTTPS streaming to the Anthropic Messages API.

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use bytes::BytesMut;
use reqwest::Client;
use tokio_stream::{Stream, StreamExt};

use crate::provider::StreamEvent;

use crate::provider::auth::AuthMethod;
use crate::provider::error::{ApiError, Error};
use crate::provider::sse::SseParser;
use crate::provider::traits::*;

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// The Messages API accepts at most four cache breakpoints per request.
const MAX_CACHE_BREAKPOINTS: usize = 4;
const API_VERSION: &str = "2023-06-01";

/// Initial capacity for the SSE line accumulator. Anthropic SSE lines are
/// typically well below 8 KiB (one JSON event per line).
const LINE_BUFFER_CAPACITY: usize = 8 * 1024;

/// Byte-level line accumulator for SSE streams.
///
/// Buffers raw bytes via `BytesMut` and only decodes to `str` once a
/// complete `\n`-terminated line has been extracted. This avoids two defects
/// of decoding each chunk with `String::from_utf8_lossy`:
///   - Multi-byte UTF-8 sequences split across TCP chunks no longer become
///     U+FFFD (the lossy replacement character).
///   - Line boundary search runs in O(k) per line via `memchr::memchr`,
///     avoiding the O(n²) behaviour of rescanning the full string buffer.
///
/// `split_to` on `BytesMut` is amortised O(1): it advances the head pointer
/// rather than copying the remaining bytes.
pub(crate) struct LineAccumulator {
    buf: BytesMut,
}

/// Longest SSE line accepted.
const MAX_LINE_BYTES: usize = 1024 * 1024;

fn line_too_long() -> Error {
    Error::SseParse {
        message: "SSE line exceeds 1 MiB".to_string(),
    }
}

impl LineAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            buf: BytesMut::with_capacity(LINE_BUFFER_CAPACITY),
        }
    }

    /// Append raw bytes received from the stream.
    pub(crate) fn extend(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Extract the next complete line, if one is available.
    ///
    /// A line is a slice terminated by `\n`. The trailing `\n` is consumed
    /// from the buffer and stripped from the returned string; a trailing
    /// `\r` (i.e. CRLF line endings) is also stripped. Returns `Ok(None)`
    /// when no complete line is buffered yet. Returns an error if the
    /// completed line is not valid UTF-8 — by contract, a line boundary
    /// (`\n`, a single ASCII byte) never splits a multi-byte codepoint,
    /// so this indicates genuinely malformed input.
    pub(crate) fn next_line(&mut self) -> Result<Option<String>, Error> {
        let Some(pos) = memchr::memchr(b'\n', &self.buf) else {
            if self.buf.len() > MAX_LINE_BYTES {
                return Err(line_too_long());
            }
            return Ok(None);
        };
        if pos > MAX_LINE_BYTES {
            return Err(line_too_long());
        }
        // Drain the line (including the trailing '\n') from the buffer in O(1).
        let line_bytes = self.buf.split_to(pos + 1);
        // Strip the terminating '\n'; then strip a trailing '\r' for CRLF.
        let mut end = pos;
        if end > 0 && line_bytes[end - 1] == b'\r' {
            end -= 1;
        }
        let line = std::str::from_utf8(&line_bytes[..end])
            .map_err(|e| Error::SseParse {
                message: format!("invalid utf-8 in line: {e}"),
            })?
            .to_string();
        Ok(Some(line))
    }

    /// Called when the upstream stream has closed. Validates that no
    /// undecodable bytes remain in the buffer. A non-empty, well-formed
    /// trailing fragment (without a terminating `\n`) is silently dropped —
    /// SSE requires blank-line-terminated events, so an unterminated
    /// fragment can never form a complete event. An invalid UTF-8 fragment
    /// at EOF is surfaced as an error rather than swallowed.
    pub(crate) fn finish(self) -> Result<(), Error> {
        if self.buf.is_empty() {
            return Ok(());
        }
        std::str::from_utf8(&self.buf)
            .map(|_| ())
            .map_err(|e| Error::SseParse {
                message: format!("invalid utf-8 on close: {e}"),
            })
    }

    /// Current backing-buffer capacity — exposed for bounded-memory
    /// verification tests.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Current buffered (undrained) byte count — exposed for tests.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }
}

/// Decode a raw byte stream (SSE transport wire) into a stream of
/// `StreamEvent`s.
///
/// This is the transport-agnostic core of the streaming pipeline —
/// extracted from the `stream()` method so it can be exercised in
/// isolation by tests that feed synthetic byte streams (e.g. the zero-byte
/// guard). The live HTTP path (`stream()`) wraps
/// `response.bytes_stream()` with an error-map and delegates here; there
/// is exactly one implementation of the SSE state machine.
///
/// Guarantees:
///   - Emits `Error::SseParse("empty response")` if the byte stream
///     terminates without yielding any non-empty chunk.
///   - Emits `Error::SseParse("invalid utf-8 …")` if a completed
///     line or trailing EOF fragment is not valid UTF-8.
///   - Emits `StreamEvent::Stop(reason)` right before `MessageStop` when
///     `message_delta` carried a stop reason.
///   - Short-circuits on the first propagated byte-stream error.
pub(crate) fn decode_sse_stream<S>(
    bytes: S,
) -> impl Stream<Item = Result<StreamEvent, Error>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, Error>> + Send + 'static,
{
    async_stream::stream! {
        let mut parser = SseParser::new();
        let mut acc = LineAccumulator::new();
        let mut received_any_bytes = false;
        let mut byte_stream = Box::pin(bytes);

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk: bytes::Bytes = match chunk_result {
                Ok(b) => b,
                Err(e) => { yield Err(e); return; }
            };

            if !chunk.is_empty() {
                received_any_bytes = true;
            }
            acc.extend(&chunk);

            loop {
                match acc.next_line() {
                    Ok(Some(line)) => match parser.feed_line(&line) {
                        Ok(Some(StreamEvent::MessageStop)) => {
                            if let Some(reason) = parser.take_stop() {
                                yield Ok(StreamEvent::Stop(reason));
                            }
                            yield Ok(StreamEvent::MessageStop);
                        }
                        Ok(Some(event)) => yield Ok(event),
                        Ok(None) => {}
                        Err(e) => { yield Err(e); return; }
                    },
                    Ok(None) => break,
                    Err(e) => { yield Err(e); return; }
                }
            }
        }

        // Guard: an HTTP 200 with zero bytes is not a valid SSE stream.
        // Surface it explicitly rather than silently terminating.
        if !received_any_bytes {
            yield Err(Error::SseParse { message: "empty response".to_string() });
            return;
        }

        // Validate any trailing partial line on stream close. A
        // well-formed but unterminated fragment is dropped (SSE events
        // require blank-line termination); undecodable bytes surface
        // as an explicit error.
        if let Err(e) = acc.finish() {
            yield Err(e);
        }
    }
}

/// Pass `bytes` through, ending with `Err(IdleTimeout { after: gap })` when
/// no item arrives for `gap`. Shared by both providers.
pub(crate) fn idle_guard<S>(
    bytes: S,
    gap: Duration,
) -> impl Stream<Item = Result<bytes::Bytes, Error>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, Error>> + Send + 'static,
{
    async_stream::stream! {
        let mut bytes = Box::pin(bytes);
        loop {
            match tokio::time::timeout(gap, bytes.next()).await {
                Ok(Some(item)) => yield item,
                Ok(None) => return,
                Err(_) => {
                    yield Err(Error::IdleTimeout { after: gap });
                    return;
                }
            }
        }
    }
}

/// HTTP client with the options' connect timeout. Shared by both providers.
pub(crate) fn build_client(options: &ProviderOptions) -> Result<Client, reqwest::Error> {
    Client::builder()
        .connect_timeout(options.connect_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// Longest error body kept from a non-2xx answer.
pub(crate) const ERROR_BODY_CAP: usize = 64 * 1024;

/// Read a body of a non-2xx answer: at most `cap` bytes, each wait under
/// `idle_gap`. A connection failure keeps the bytes read so far; a trailing
/// incomplete UTF-8 sequence is dropped.
pub(crate) async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
    idle_gap: Duration,
) -> Result<String, Error> {
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < cap {
        match tokio::time::timeout(idle_gap, response.chunk()).await {
            Err(_) => return Err(Error::IdleTimeout { after: idle_gap }),
            Ok(Ok(Some(chunk))) => {
                let room = cap - buf.len();
                buf.extend_from_slice(&chunk[..chunk.len().min(room)]);
            }
            Ok(Ok(None)) | Ok(Err(_)) => break,
        }
    }
    if let Err(e) = std::str::from_utf8(&buf) {
        if e.error_len().is_none() {
            buf.truncate(e.valid_up_to());
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Check `req.cache_breakpoints` against the request shape. Shared by the
/// streaming path and batch submission.
pub(crate) fn validate_breakpoints(req: &CompletionRequest) -> Result<(), Error> {
    let invalid = |msg: String| Err(Error::InvalidRequest(msg));
    let breakpoints = &req.cache_breakpoints;
    if breakpoints.len() > MAX_CACHE_BREAKPOINTS {
        return invalid(format!(
            "too many cache breakpoints: {} (max {MAX_CACHE_BREAKPOINTS})",
            breakpoints.len()
        ));
    }
    for (n, bp) in breakpoints.iter().enumerate() {
        match bp.position {
            BreakpointPosition::Message(i) => match req.messages.get(i) {
                None => return invalid(format!("cache breakpoint Message({i}) out of range")),
                Some(m) if m.content.is_empty() => {
                    return invalid(format!(
                        "cache breakpoint Message({i}) points at an empty message"
                    ))
                }
                Some(_) => {}
            },
            BreakpointPosition::System if req.system.is_none() => {
                return invalid("cache breakpoint System without a system prompt".to_string())
            }
            BreakpointPosition::Tools if req.tools.is_empty() => {
                return invalid("cache breakpoint Tools without tools".to_string())
            }
            _ => {}
        }
        if breakpoints[..n].iter().any(|b| b.position == bp.position) {
            return invalid(format!("duplicate cache breakpoint {:?}", bp.position));
        }
    }
    // Longer TTLs must come first in the order tools → system → messages.
    let rank = |position: BreakpointPosition| match position {
        BreakpointPosition::Tools => 0,
        BreakpointPosition::System => 1,
        BreakpointPosition::Message(i) => 2 + i,
    };
    let ranks_of = |ttl: CacheTtl| {
        breakpoints
            .iter()
            .filter(move |b| b.ttl == ttl)
            .map(move |b| rank(b.position))
    };
    if let Some(first_short) = ranks_of(CacheTtl::FiveMinutes).min() {
        if ranks_of(CacheTtl::OneHour).any(|r| r > first_short) {
            return invalid("1h cache breakpoint after a 5m breakpoint".to_string());
        }
    }
    Ok(())
}

/// Wire form of a breakpoint's `cache_control`. The 1-hour TTL needs no
/// beta header.
fn cache_control(ttl: CacheTtl) -> serde_json::Value {
    match ttl {
        CacheTtl::FiveMinutes => serde_json::json!({ "type": "ephemeral" }),
        CacheTtl::OneHour => serde_json::json!({ "type": "ephemeral", "ttl": "1h" }),
    }
}

/// Anthropic Messages API provider with SSE streaming.
pub struct AnthropicApi {
    client: Client,
    model: String,
    auth: AuthMethod,
    /// API origin without trailing `/`; requests go to `{base_url}/v1/messages`.
    base_url: String,
    /// Longest silence before headers or between body chunks.
    idle_gap: Duration,
}

impl AnthropicApi {
    /// Create a provider with explicit auth, API origin and timeouts.
    pub fn with_options(
        model: &str,
        auth: AuthMethod,
        base_url: &str,
        options: ProviderOptions,
    ) -> Result<Self, Error> {
        let client = build_client(&options)?;
        Ok(Self::from_parts(client, model, auth, base_url, options))
    }

    fn from_parts(
        client: Client,
        model: &str,
        auth: AuthMethod,
        base_url: &str,
        options: ProviderOptions,
    ) -> Self {
        Self {
            client,
            model: model.to_string(),
            auth,
            base_url: base_url.trim_end_matches('/').to_string(),
            idle_gap: options.idle_gap,
        }
    }

    /// Create a provider with explicit auth (useful for testing).
    pub fn with_auth(model: &str, auth: AuthMethod) -> Self {
        Self::with_base_url(model, auth, DEFAULT_BASE_URL)
    }

    /// Create a provider with explicit auth and API origin (e.g. a proxy or
    /// a local test server). A trailing `/` on `base_url` is ignored.
    pub fn with_base_url(model: &str, auth: AuthMethod, base_url: &str) -> Self {
        let options = ProviderOptions::default();
        let client = build_client(&options).expect("HTTP client");
        Self::from_parts(client, model, auth, base_url, options)
    }

    /// API origin without trailing `/`.
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Longest silence allowed on any response.
    pub(crate) fn idle_gap(&self) -> Duration {
        self.idle_gap
    }

    /// A request carrying `anthropic-version` and the auth headers: `x-api-key`
    /// for an API key; Bearer plus `anthropic-beta: oauth-2025-04-20` for an
    /// OAuth token. The single place all Anthropic header logic lives.
    pub(crate) fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let req = self
            .client
            .request(method, url)
            .header("anthropic-version", API_VERSION);
        match &self.auth {
            AuthMethod::ApiKey(key) => req.header("x-api-key", key),
            AuthMethod::Bearer(token) => req
                .bearer_auth(token)
                .header("anthropic-beta", "oauth-2025-04-20"),
        }
    }

    pub(crate) fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        // Resolve `ModelSelection` to a concrete model id for the
        // wire payload. `Default` → provider's configured model;
        // `Named(id)` → caller-supplied override.
        let effective_model: String = match &request.model {
            ModelSelection::Default => self.model.clone(),
            ModelSelection::Named(id) => id.clone(),
        };
        // `request.messages` is `Arc<[Message]>` — borrow a slice for iteration.
        let messages_slice: &[Message] = &request.messages;
        let messages: Vec<serde_json::Value> = messages_slice
            .iter()
            .map(|m| {
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .map(|c| match c {
                        Content::Text { text } => {
                            serde_json::json!({ "type": "text", "text": text })
                        }
                        Content::ToolUse { id, name, input } => {
                            serde_json::json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input
                            })
                        }
                        Content::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            serde_json::json!({
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": content,
                                "is_error": is_error
                            })
                        }
                    })
                    .collect();

                serde_json::json!({
                    "role": match m.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                    },
                    "content": content
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": effective_model,
            "max_tokens": request.max_tokens,
            "messages": messages
        });

        if let Some(system) = &request.system {
            body["system"] = serde_json::json!(system);
        }

        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": &t.name,
                        "description": &t.description,
                        "input_schema": &t.input_schema
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        // Breakpoints were validated by the caller (`stream()`, batch `submit`); a position that does not
        // resolve here is left unrendered rather than panicking.
        for bp in &request.cache_breakpoints {
            let marker = cache_control(bp.ttl);
            match bp.position {
                BreakpointPosition::Tools => {
                    if let Some(last) = body["tools"].as_array_mut().and_then(|t| t.last_mut()) {
                        last["cache_control"] = marker;
                    }
                }
                BreakpointPosition::System => {
                    if let Some(system) = &request.system {
                        body["system"] = serde_json::json!([{
                            "type": "text",
                            "text": system,
                            "cache_control": marker
                        }]);
                    }
                }
                BreakpointPosition::Message(i) => {
                    if let Some(last) = body["messages"]
                        .get_mut(i)
                        .and_then(|m| m["content"].as_array_mut())
                        .and_then(|c| c.last_mut())
                    {
                        last["cache_control"] = marker;
                    }
                }
            }
        }

        body
    }
}

#[async_trait]
impl LlmProvider for AnthropicApi {
    fn stream(
        &self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>> {
        if let Err(e) = validate_breakpoints(&request) {
            return Box::pin(tokio_stream::once(Err(e)));
        }
        let mut body = self.build_request_body(&request);
        body["stream"] = serde_json::json!(true);
        let request_future = self
            .request(
                reqwest::Method::POST,
                &format!("{}/v1/messages", self.base_url),
            )
            .header("content-type", "application/json")
            .json(&body)
            .send();

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
                    Ok(text) => yield Err(Error::Api(ApiError::from_response(status, &headers, &text))),
                    Err(e) => yield Err(e),
                }
                return;
            }

            let byte_stream = idle_guard(
                response.bytes_stream().map(|r| r.map_err(Error::Http)),
                gap,
            );

            // Delegate to the transport-agnostic SSE decoder. Same code
            // path is exercised by `c08_a12_empty_body_*` tests below,
            // which feed synthetic byte streams to verify the zero-byte
            // guard and other stream-level invariants.
            let mut events = Box::pin(decode_sse_stream(byte_stream));
            while let Some(ev) = events.next().await {
                yield ev;
            }
        })
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "anthropic".to_string(),
            model: self.model.clone(),
            supports_tools: true,
            supports_streaming: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::StopReason;
    use std::sync::Arc;

    #[test]
    fn build_request_body_basic() {
        let provider = AnthropicApi::with_auth(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("test-key".to_string()),
        );

        let request = CompletionRequest {
            model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
            system: Some("You are helpful.".to_string()),
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hello".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 1024,
            cache_breakpoints: Vec::new(),
        };

        let body = provider.build_request_body(&request);

        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 1024);
        assert!(body.get("stream").is_none());
        assert_eq!(body["system"], "You are helpful.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][0]["text"], "Hello");
        // No tools key when tools is empty
        assert!(body.get("tools").is_none());
    }

    /// Without breakpoints the wire body is byte-identical to the
    /// plain encoding (golden recorded from the unbroken-up request).
    #[test]
    fn no_breakpoints_body_matches_pre_change_golden() {
        let provider = AnthropicApi::with_auth(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("test-key".to_string()),
        );
        let request = CompletionRequest {
            model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
            system: Some("You are helpful.".to_string()),
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hello".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 1024,
            cache_breakpoints: vec![],
        };
        let golden = r#"{"max_tokens":1024,"messages":[{"content":[{"text":"Hello","type":"text"}],"role":"user"}],"model":"claude-sonnet-4-6","system":"You are helpful."}"#;
        let encoded =
            serde_json::to_string(&provider.build_request_body(&request)).expect("body serializes");
        assert_eq!(encoded, golden);
    }

    /// System/OneHour turns `system` into a cached text block;
    /// Message(0)/FiveMinutes marks the last block of message 0.
    #[test]
    fn breakpoints_render_cache_control() {
        let provider =
            AnthropicApi::with_auth("claude-sonnet-4-6", AuthMethod::ApiKey("k".to_string()));
        let request = CompletionRequest {
            model: ModelSelection::Default,
            system: Some("S".to_string()),
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: vec![
                CacheBreakpoint {
                    position: BreakpointPosition::System,
                    ttl: CacheTtl::OneHour,
                },
                CacheBreakpoint {
                    position: BreakpointPosition::Message(0),
                    ttl: CacheTtl::FiveMinutes,
                },
            ],
        };
        let body = provider.build_request_body(&request);
        assert_eq!(
            body["system"],
            serde_json::json!([{
                "type": "text",
                "text": "S",
                "cache_control": {"type": "ephemeral", "ttl": "1h"}
            }])
        );
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"],
            serde_json::json!({"type": "ephemeral"})
        );
    }

    /// Five breakpoints fail locally: the stream's only item is
    /// `InvalidRequest`, and the server never sees a connection.
    #[tokio::test]
    async fn too_many_breakpoints_fails_before_connecting() {
        let stub = crate::provider::test_stub::StubServer::start(vec![]).await;
        let provider = AnthropicApi::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("k".to_string()),
            &stub.url(),
        );
        let bp = |position| CacheBreakpoint {
            position,
            ttl: CacheTtl::FiveMinutes,
        };
        let request = CompletionRequest {
            model: ModelSelection::Default,
            system: Some("S".to_string()),
            messages: Arc::from(vec![
                Message {
                    role: MessageRole::User,
                    content: vec![Content::Text {
                        text: "a".to_string(),
                    }],
                },
                Message {
                    role: MessageRole::Assistant,
                    content: vec![Content::Text {
                        text: "b".to_string(),
                    }],
                },
                Message {
                    role: MessageRole::User,
                    content: vec![Content::Text {
                        text: "c".to_string(),
                    }],
                },
            ]),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: vec![
                bp(BreakpointPosition::System),
                bp(BreakpointPosition::Message(0)),
                bp(BreakpointPosition::Message(1)),
                bp(BreakpointPosition::Message(2)),
                bp(BreakpointPosition::Message(3)),
            ],
        };
        let items: Vec<_> = provider.stream(request).collect().await;
        assert_eq!(items.len(), 1, "expected exactly one item, got {:?}", items);
        match &items[0] {
            Err(Error::InvalidRequest(msg)) => {
                assert_eq!(msg, "too many cache breakpoints: 5 (max 4)")
            }
            other => panic!("expected InvalidRequest, got {:?}", other),
        }
        assert_eq!(stub.connection_count(), 0);
    }

    /// Validation table — every remaining rejection message.
    #[test]
    fn validate_breakpoints_messages() {
        let bp = |position| CacheBreakpoint {
            position,
            ttl: CacheTtl::FiveMinutes,
        };
        let req = |messages: Vec<Message>, breakpoints: Vec<CacheBreakpoint>| CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: Arc::from(messages),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: breakpoints,
        };
        let user = |content: Vec<Content>| Message {
            role: MessageRole::User,
            content,
        };
        let hi = || {
            vec![Content::Text {
                text: "hi".to_string(),
            }]
        };
        let cases = vec![
            (
                req(vec![user(hi())], vec![bp(BreakpointPosition::Message(1))]),
                "cache breakpoint Message(1) out of range",
            ),
            (
                req(vec![user(vec![])], vec![bp(BreakpointPosition::Message(0))]),
                "cache breakpoint Message(0) points at an empty message",
            ),
            (
                req(vec![user(hi())], vec![bp(BreakpointPosition::System)]),
                "cache breakpoint System without a system prompt",
            ),
            (
                req(vec![user(hi())], vec![bp(BreakpointPosition::Tools)]),
                "cache breakpoint Tools without tools",
            ),
            (
                req(
                    vec![user(hi())],
                    vec![
                        bp(BreakpointPosition::Message(0)),
                        bp(BreakpointPosition::Message(0)),
                    ],
                ),
                "duplicate cache breakpoint Message(0)",
            ),
        ];
        for (request, expected) in cases {
            match validate_breakpoints(&request) {
                Err(Error::InvalidRequest(msg)) => assert_eq!(msg, expected),
                other => panic!("expected InvalidRequest({expected:?}), got {other:?}"),
            }
        }
        assert!(validate_breakpoints(&req(vec![user(hi())], vec![])).is_ok());
    }

    /// A Tools breakpoint marks only the last tool definition.
    #[test]
    fn tools_breakpoint_marks_last_tool() {
        let provider =
            AnthropicApi::with_auth("claude-sonnet-4-6", AuthMethod::ApiKey("k".to_string()));
        let tool = |name: &str| ToolDefinition {
            name: name.to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let request = CompletionRequest {
            model: ModelSelection::Default,
            system: Some("S".to_string()),
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }]),
            tools: vec![tool("a"), tool("b")],
            max_tokens: 16,
            cache_breakpoints: vec![CacheBreakpoint {
                position: BreakpointPosition::Tools,
                ttl: CacheTtl::OneHour,
            }],
        };
        let body = provider.build_request_body(&request);
        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(
            body["tools"][1]["cache_control"],
            serde_json::json!({"type": "ephemeral", "ttl": "1h"})
        );
        assert_eq!(
            body["system"], "S",
            "no System breakpoint keeps system a string"
        );
    }

    #[test]
    fn build_request_body_with_tools() {
        let provider = AnthropicApi::with_auth(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("test-key".to_string()),
        );

        let request = CompletionRequest {
            model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "List files".to_string(),
                }],
            }]),
            tools: vec![ToolDefinition {
                name: "bash".to_string(),
                description: "Run a command".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}}),
            }],
            max_tokens: 4096,
            cache_breakpoints: Vec::new(),
        };

        let body = provider.build_request_body(&request);

        assert!(body.get("system").is_none());
        let tools = body["tools"].as_array().expect("tools should be array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "bash");
    }

    #[test]
    fn build_request_body_with_tool_result() {
        let provider = AnthropicApi::with_auth(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("test-key".to_string()),
        );

        let request = CompletionRequest {
            model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
            system: None,
            messages: Arc::from(vec![
                Message {
                    role: MessageRole::User,
                    content: vec![Content::Text {
                        text: "List files".to_string(),
                    }],
                },
                Message {
                    role: MessageRole::Assistant,
                    content: vec![Content::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "bash".to_string(),
                        input: serde_json::json!({"command": "ls"}),
                    }],
                },
                Message {
                    role: MessageRole::User,
                    content: vec![Content::ToolResult {
                        tool_use_id: "toolu_1".to_string(),
                        content: "file.txt".to_string(),
                        is_error: false,
                    }],
                },
            ]),
            tools: vec![],
            max_tokens: 1024,
            cache_breakpoints: Vec::new(),
        };

        let body = provider.build_request_body(&request);

        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][0]["id"], "toolu_1");
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(body["messages"][2]["content"][0]["is_error"], false);
    }

    #[test]
    fn provider_info() {
        let provider =
            AnthropicApi::with_auth("claude-sonnet-4-6", AuthMethod::ApiKey("k".to_string()));
        let info = provider.info();
        assert_eq!(info.name, "anthropic");
        assert_eq!(info.model, "claude-sonnet-4-6");
        assert!(info.supports_tools);
        assert!(info.supports_streaming);
    }

    // --- ModelSelection resolution at request-build time ---

    /// `ModelSelection::Default` asks the provider to use its
    /// configured model. `AnthropicApi::with_auth("opus", …)` with
    /// `request.model: Default` must send `"opus"` in the wire payload.
    #[test]
    fn model_selection_default_uses_provider_configured_model() {
        let provider = AnthropicApi::with_auth("opus", AuthMethod::ApiKey("k".to_string()));
        let request = CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "hi".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: Vec::new(),
        };
        let body = provider.build_request_body(&request);
        assert_eq!(
            body["model"], "opus",
            "Default must resolve to provider's configured model"
        );
    }

    /// `ModelSelection::Named(id)` overrides the provider's
    /// configured model — the named id appears verbatim on the wire.
    #[test]
    fn model_selection_named_overrides_provider_configured_model() {
        let provider = AnthropicApi::with_auth("opus", AuthMethod::ApiKey("k".to_string()));
        let request = CompletionRequest {
            model: ModelSelection::Named("sonnet".to_string()),
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "hi".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: Vec::new(),
        };
        let body = provider.build_request_body(&request);
        assert_eq!(
            body["model"], "sonnet",
            "Named override must replace the provider's configured model"
        );
    }

    /// No model sentinel built from `String::new()` remains in this
    /// crate's source.
    ///
    /// The scan excludes this very test file (which necessarily references
    /// the pattern in literal strings); everything else must be clean.
    ///
    /// `#[cfg(unix)]` because the test shells out to `grep`: Linux and macOS
    /// only, Windows is unsupported.
    #[cfg(unix)]
    #[test]
    fn no_string_new_model_sentinel_in_src() {
        use std::process::Command;
        // Scan this crate's source.
        let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        // The needle is split across two substrings so this source line
        // itself does not form the forbidden pattern.
        let needle = format!("model: {}::new()", "String");
        let out = Command::new("grep")
            .arg("-rn")
            .arg(&needle)
            .arg(src_dir)
            .output()
            .expect("grep must be available on PATH");
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Filter out any match lines from this very test file — they
        // appear in string literals / comments and are not actual
        // sentinels. `file!()` gives the relative path the compiler
        // recorded; match on its basename.
        let this_file = std::path::Path::new(file!())
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let real_hits: Vec<&str> = stdout.lines().filter(|l| !l.contains(this_file)).collect();
        assert!(
            real_hits.is_empty(),
            "no `model: String::new()` sentinels must remain; found:\n{}",
            real_hits.join("\n")
        );
    }

    // --- UTF-8 split across TCP chunks ("Müller") ---

    /// `Müller` delivered in two chunks, splitting the 2-byte `ü`
    /// (0xC3 0xBC) across the chunk boundary. The BytesMut accumulator
    /// must not produce U+FFFD and must reconstruct the exact line.
    #[test]
    fn utf8_split_across_chunks_mueller() {
        let mut acc = LineAccumulator::new();
        // Chunk 1: "M" + first byte of ü (0xC3), no newline yet.
        acc.extend(&[b'M', 0xC3]);
        assert!(
            acc.next_line()
                .expect("no decode error mid-chunk")
                .is_none(),
            "no line should be available before newline arrives"
        );
        // Chunk 2: second byte of ü (0xBC) + "ller" + '\n'
        acc.extend(&[0xBC, b'l', b'l', b'e', b'r', b'\n']);
        let line = acc
            .next_line()
            .expect("must decode once complete")
            .expect("line must be available after newline");
        assert_eq!(line, "Müller");
        assert!(!line.contains('\u{FFFD}'), "must not contain U+FFFD");
    }

    // --- 4-byte emoji "🚀" split at offsets 1, 2, 3 ---

    /// The rocket emoji `🚀` is U+1F680 = `F0 9F 9A 80` (4 bytes in
    /// UTF-8). Splitting the byte stream at each of offsets 1, 2, 3 must
    /// still reconstruct the emoji exactly — proving the accumulator does
    /// not depend on chunk alignment.
    #[test]
    fn utf8_split_four_byte_emoji_all_offsets() {
        let emoji = [0xF0, 0x9F, 0x9A, 0x80, b'\n']; // "🚀\n"
        for split in 1..=3 {
            let mut acc = LineAccumulator::new();
            acc.extend(&emoji[..split]);
            assert!(
                acc.next_line().expect("no error mid-codepoint").is_none(),
                "split={}: no line should be available mid-codepoint",
                split
            );
            acc.extend(&emoji[split..]);
            let line = acc
                .next_line()
                .expect("must decode once complete")
                .expect("line must be available after newline");
            assert_eq!(line, "🚀", "split at offset {} must round-trip", split);
            assert!(
                !line.contains('\u{FFFD}'),
                "split={}: must not contain U+FFFD",
                split
            );
        }
    }

    // --- Japanese "漢字" split mid-codepoint ---

    /// `漢` = U+6F22 = `E6 BC A2`, `字` = U+5B57 = `E5 AD 97`. Split
    /// inside the first codepoint (between the first two bytes of `漢`).
    #[test]
    fn utf8_split_japanese_mid_codepoint() {
        let mut acc = LineAccumulator::new();
        // Chunk 1: first byte of 漢
        acc.extend(&[0xE6]);
        assert!(acc.next_line().expect("no error").is_none());
        // Chunk 2: remaining 2 bytes of 漢 + first byte of 字
        acc.extend(&[0xBC, 0xA2, 0xE5]);
        assert!(acc.next_line().expect("no error").is_none());
        // Chunk 3: remaining 2 bytes of 字 + '\n'
        acc.extend(&[0xAD, 0x97, b'\n']);
        let line = acc
            .next_line()
            .expect("must decode once complete")
            .expect("line must be available after newline");
        assert_eq!(line, "漢字");
        assert!(!line.contains('\u{FFFD}'), "must not contain U+FFFD");
    }

    // --- bounded buffer under 100 MB stream ---

    /// Feed ~100 MB of line-structured SSE content in ~64 KiB chunks
    /// and assert that the `BytesMut` capacity never grows beyond a small
    /// bound. This is the proxy for RSS growth:
    /// the memory-bounded invariant we actually care about is that each
    /// completed line drains via `split_to`, releasing the head, so the
    /// buffer's live bytes stay O(longest-line), not O(total-stream).
    ///
    /// We sample capacity both *before* draining completed lines (when it
    /// is at its highest, since a full chunk has just been appended) and
    /// *after* draining (when `split_to` has advanced the head past every
    /// extracted line). The operative invariant is that `peak_before_drain`
    /// does not grow with total bytes processed — it stays at roughly one
    /// TCP chunk's worth, regardless of whether we feed 1 MB or 100 MB.
    #[test]
    fn stream_buffer_stays_bounded() {
        // Generous upper bound: allocator doubling from the 8 KiB initial
        // capacity to hold a 64 KiB chunk lands on 64 KiB exactly on
        // current `bytes` crate policy, but future versions may grow to
        // the next power of two. 128 KiB leaves a 2x safety margin.
        const CAP_BOUND: usize = 128 * 1024;
        const TOTAL: usize = 100 * 1024 * 1024; // 100 MB
        const LINE_LEN: usize = 1024;
        const CHUNK_LEN: usize = 64 * 1024;

        // Construct a single reusable chunk of CHUNK_LEN bytes containing
        // whole LINE_LEN-terminated lines (CHUNK_LEN is a multiple of LINE_LEN).
        assert_eq!(CHUNK_LEN % LINE_LEN, 0);
        let mut chunk = Vec::with_capacity(CHUNK_LEN);
        let payload_len = LINE_LEN - 1; // minus '\n'
        for _ in 0..(CHUNK_LEN / LINE_LEN) {
            chunk.extend(std::iter::repeat_n(b'x', payload_len));
            chunk.push(b'\n');
        }
        assert_eq!(chunk.len(), CHUNK_LEN);

        let mut acc = LineAccumulator::new();
        let mut peak_before_drain = 0usize;
        let mut lines_drained = 0usize;
        let mut bytes_fed = 0usize;

        while bytes_fed < TOTAL {
            acc.extend(&chunk);
            bytes_fed += CHUNK_LEN;
            // Sample capacity at its high-water mark (post-extend, pre-drain).
            peak_before_drain = peak_before_drain.max(acc.capacity());
            // Drain every complete line — emulates the runtime loop.
            while let Some(_line) = acc.next_line().expect("well-formed stream") {
                lines_drained += 1;
            }
            // After draining an exact multiple of lines, residual is 0.
            assert_eq!(
                acc.len(),
                0,
                "aligned chunk must fully drain to zero residual"
            );
        }

        assert_eq!(lines_drained, TOTAL / LINE_LEN);
        assert!(
            peak_before_drain <= CAP_BOUND,
            "buffer capacity must stay bounded regardless of stream length; \
             peak_before_drain={} bound={}",
            peak_before_drain,
            CAP_BOUND
        );
    }

    // --- CRLF line endings ---

    /// SSE permits `\r\n` line endings. The accumulator must strip
    /// the `\r` so the downstream parser sees clean line content.
    #[test]
    fn crlf_line_endings_stripped() {
        let mut acc = LineAccumulator::new();
        acc.extend(b"event: message_stop\r\n");
        let line = acc.next_line().unwrap().expect("line available");
        assert_eq!(line, "event: message_stop");
        assert!(!line.ends_with('\r'));
    }

    // --- connection closes with 0 bytes ---

    /// When the remote closes the connection without sending any
    /// bytes, `LineAccumulator::finish` must be a no-op (no panic, no
    /// spurious error). The zero-bytes detection happens in the stream
    /// loop via `received_any_bytes`, emitting `Error::SseParse`.
    /// Here we pin down the accumulator contract.
    #[test]
    fn empty_buffer_finish_no_error() {
        let acc = LineAccumulator::new();
        assert!(
            acc.finish().is_ok(),
            "empty buffer must close without error"
        );
    }

    /// A trailing well-formed UTF-8 fragment without terminating
    /// `\n` is silently dropped (SSE requires blank-line termination, so
    /// no event could ever form from it). Invalid UTF-8 at close surfaces
    /// as `Error::SseParse`.
    #[test]
    fn trailing_partial_line_close() {
        // Well-formed partial line: dropped.
        let mut acc = LineAccumulator::new();
        acc.extend(b"event: partial");
        assert!(acc.finish().is_ok());

        // Invalid UTF-8 at close: error.
        let mut acc = LineAccumulator::new();
        acc.extend(&[0xC3]); // first byte of a 2-byte sequence, no follow-up
        match acc.finish() {
            Err(Error::SseParse { message: msg }) => {
                assert!(
                    msg.contains("invalid utf-8"),
                    "message must identify utf-8 issue, got: {}",
                    msg
                );
            }
            other => panic!("expected Stream error on invalid utf-8, got {:?}", other),
        }
    }

    // --- HTTP 200 with empty body ---
    //
    // The zero-byte guard in `decode_sse_stream` yields
    // `Error::SseParse("empty response")` when the upstream byte
    // stream terminates without producing any data. These tests exercise
    // the actual code path by feeding synthetic empty byte streams into
    // the helper — the same helper the live HTTP path delegates to from
    // `stream()`. A future refactor that removes or short-circuits the
    // guard fails here.

    /// Completely empty stream (no chunks at all).
    #[tokio::test]
    async fn decode_sse_empty_stream_yields_empty_response() {
        use tokio_stream::StreamExt as _;

        // Empty byte stream — no chunks emitted.
        let bytes = async_stream::stream! {
            // intentionally empty: immediately terminates
            if false {
                yield Ok::<bytes::Bytes, Error>(bytes::Bytes::new());
            }
        };
        let mut events = Box::pin(decode_sse_stream(bytes));

        let first = events.next().await.expect("expected one error event");
        match first {
            Err(Error::SseParse { message }) => {
                assert_eq!(message, "empty response");
            }
            other => panic!("expected SseParse(\"empty response\"), got {:?}", other),
        }
        assert!(
            events.next().await.is_none(),
            "guard must terminate the stream after emitting the error"
        );
    }

    /// Stream emits only empty `Bytes` chunks (degenerate upstream).
    ///
    /// Same observable outcome as the no-chunks case: `received_any_bytes`
    /// never flips true, so the guard fires. Pins the semantics of "no
    /// *data* bytes received" vs "no chunks at all".
    #[tokio::test]
    async fn decode_sse_only_empty_chunks_yields_empty_response() {
        use tokio_stream::StreamExt as _;

        let bytes = async_stream::stream! {
            yield Ok::<bytes::Bytes, Error>(bytes::Bytes::new());
            yield Ok::<bytes::Bytes, Error>(bytes::Bytes::new());
        };
        let mut events = Box::pin(decode_sse_stream(bytes));

        let first = events.next().await.expect("expected one error event");
        match first {
            Err(Error::SseParse { message }) => {
                assert_eq!(message, "empty response");
            }
            other => panic!("expected SseParse(\"empty response\"), got {:?}", other),
        }
        assert!(events.next().await.is_none());
    }

    /// Negative case: a non-empty stream must NOT trigger the
    /// empty-body guard, AND must produce the expected event end-to-end.
    /// Guards against a future refactor that fires the error unconditionally
    /// OR silently drops well-formed events.
    #[tokio::test]
    async fn decode_sse_non_empty_stream_does_not_trigger_empty_guard() {
        use tokio_stream::StreamExt as _;

        // A single well-formed SSE event: event line + data line + blank.
        // Concatenated string literal avoids indentation artifacts in the bytes.
        let payload: &[u8] = b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let bytes = async_stream::stream! {
            yield Ok::<bytes::Bytes, Error>(bytes::Bytes::from_static(payload));
        };
        let mut events = Box::pin(decode_sse_stream(bytes));

        let mut saw_message_stop = false;
        while let Some(ev) = events.next().await {
            match ev {
                Ok(crate::provider::StreamEvent::MessageStop) => saw_message_stop = true,
                Err(Error::SseParse { message: msg }) if msg == "empty response" => {
                    panic!("empty-body guard fired on a non-empty stream: {msg}");
                }
                Ok(_) => {} // other events are fine (ignore)
                Err(e) => panic!("unexpected error on well-formed stream: {e}"),
            }
        }
        assert!(
            saw_message_stop,
            "well-formed SSE payload must produce a MessageStop event"
        );
    }

    // --- stop reason, TTL order ---

    fn sse_body(stop_reason: &str) -> String {
        format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"usage\":{{\"input_tokens\":3}}}}}}\n\n\
             event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"1, 2\"}}}}\n\n\
             event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":5}}}}\n\n\
             event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
        )
    }

    async fn decode_all(body: String) -> Vec<StreamEvent> {
        let bytes = tokio_stream::iter(vec![Ok::<bytes::Bytes, Error>(bytes::Bytes::from(body))]);
        decode_sse_stream(bytes)
            .map(|r| r.expect("no error expected"))
            .collect()
            .await
    }

    fn text_request(breakpoints: Vec<CacheBreakpoint>) -> CompletionRequest {
        CompletionRequest {
            model: ModelSelection::Default,
            system: Some("S".to_string()),
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Count from 1 to 50".to_string(),
                }],
            }]),
            tools: vec![ToolDefinition {
                name: "t".to_string(),
                description: "d".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            max_tokens: 5,
            cache_breakpoints: breakpoints,
        }
    }

    fn bp(position: BreakpointPosition, ttl: CacheTtl) -> CacheBreakpoint {
        CacheBreakpoint { position, ttl }
    }

    /// `complete()` sends `"stream": true` on the wire even though the
    /// request carries no flag.
    #[tokio::test]
    async fn complete_sends_stream_true() {
        let stub = crate::provider::test_stub::StubServer::start(vec![(
            ("POST", "/v1/messages"),
            (
                200,
                "text/event-stream",
                include_str!("../../tests/fixtures/anthropic_text.sse").to_string(),
            ),
        )])
        .await;
        let provider = AnthropicApi::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("sk-test".to_string()),
            &stub.url(),
        );
        let request = CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: Vec::new(),
        };
        provider.complete(request).await.expect("complete");
        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_str(&requests[0].body).expect("json body");
        assert_eq!(body["stream"], true);
    }

    /// `message_delta` with `max_tokens` makes `complete().stop`
    /// `MaxTokens`.
    #[tokio::test]
    async fn complete_reports_max_tokens() {
        let stub = crate::provider::test_stub::StubServer::start(vec![(
            ("POST", "/v1/messages"),
            (200, "text/event-stream", sse_body("max_tokens")),
        )])
        .await;
        let provider = AnthropicApi::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("k".to_string()),
            &stub.url(),
        );
        let completion = provider
            .complete(text_request(vec![]))
            .await
            .expect("complete");
        assert_eq!(completion.stop, StopReason::MaxTokens);
        assert_eq!(completion.text, "1, 2");
    }

    /// Every Anthropic stop value; `Stop` is second-to-last and
    /// `MessageStop` last.
    #[tokio::test]
    async fn anthropic_stream_stop_table() {
        for (wire, want) in [
            ("end_turn", StopReason::EndTurn),
            ("tool_use", StopReason::ToolUse),
            ("max_tokens", StopReason::MaxTokens),
            ("stop_sequence", StopReason::StopSequence),
            ("refusal", StopReason::Refusal),
            ("pause_turn", StopReason::PauseTurn),
            ("brand_new", StopReason::Unknown),
        ] {
            let events = decode_all(sse_body(wire)).await;
            let n = events.len();
            assert!(n >= 2, "{wire}: {events:?}");
            assert!(
                matches!(events[n - 2], StreamEvent::Stop(r) if r == want),
                "{wire}: {events:?}"
            );
            assert!(matches!(events[n - 1], StreamEvent::MessageStop));
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, StreamEvent::Stop(_)))
                    .count(),
                1
            );
        }
    }

    /// A 1h breakpoint after a 5m one fails before any connection.
    #[tokio::test]
    async fn one_hour_after_five_minutes_fails_before_connecting() {
        let stub = crate::provider::test_stub::StubServer::start(vec![]).await;
        let provider = AnthropicApi::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("k".to_string()),
            &stub.url(),
        );
        let request = text_request(vec![
            bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
            bp(BreakpointPosition::Message(0), CacheTtl::OneHour),
        ]);
        let items: Vec<_> = provider.stream(request).collect().await;
        assert_eq!(items.len(), 1, "got {items:?}");
        match &items[0] {
            Err(Error::InvalidRequest(msg)) => {
                assert_eq!(msg, "1h cache breakpoint after a 5m breakpoint")
            }
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
        assert_eq!(stub.connection_count(), 0);
    }

    /// 1h before 5m is fine.
    #[test]
    fn one_hour_before_five_minutes_is_ok() {
        let request = text_request(vec![
            bp(BreakpointPosition::Tools, CacheTtl::OneHour),
            bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
        ]);
        assert!(validate_breakpoints(&request).is_ok());
    }

    /// The check uses position rank, not vector order.
    #[test]
    fn rank_not_vector_order() {
        let request = text_request(vec![
            bp(BreakpointPosition::Message(0), CacheTtl::OneHour),
            bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
        ]);
        match validate_breakpoints(&request) {
            Err(Error::InvalidRequest(msg)) => {
                assert_eq!(msg, "1h cache breakpoint after a 5m breakpoint")
            }
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    // --- idle gap (real time, 1 s; paused time raced the stub's socket) ---

    async fn collect_idle(
        body: crate::provider::test_stub::StubBody,
    ) -> Vec<Result<StreamEvent, Error>> {
        let stub = crate::provider::test_stub::StubServer::start_replies(vec![(
            ("POST", "/v1/messages"),
            vec![crate::provider::test_stub::StubReply {
                status: 200,
                content_type: "text/event-stream",
                headers: Vec::new(),
                body,
            }],
        )])
        .await;
        let provider = AnthropicApi::with_options(
            "m",
            AuthMethod::ApiKey("test-key".to_string()),
            &stub.url(),
            ProviderOptions {
                connect_timeout: Duration::from_secs(30),
                idle_gap: Duration::from_secs(1),
            },
        )
        .expect("provider");
        let started = std::time::Instant::now();
        let items: Vec<_> = provider.stream(text_request(vec![])).collect().await;
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "took {:?}",
            started.elapsed()
        );
        items
    }

    fn assert_idle_after_1s(items: &[Result<StreamEvent, Error>]) {
        assert_eq!(items.len(), 1, "items: {items:?}");
        assert!(
            matches!(&items[0], Err(Error::IdleTimeout { after }) if *after == Duration::from_secs(1)),
            "got {:?}",
            items[0]
        );
    }

    /// Headers arrive, then silence: `IdleTimeout` after 1 s.
    #[tokio::test]
    async fn idle_gap_between_chunks() {
        let items = collect_idle(crate::provider::test_stub::StubBody::HeadersThenHang).await;
        assert_idle_after_1s(&items);
    }

    /// No headers at all: `IdleTimeout` after 1 s.
    #[tokio::test]
    async fn idle_gap_before_headers() {
        let items = collect_idle(crate::provider::test_stub::StubBody::Hang).await;
        assert_idle_after_1s(&items);
    }

    fn hi_request() -> CompletionRequest {
        CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }]),
            tools: vec![],
            max_tokens: 64,
            cache_breakpoints: Vec::new(),
        }
    }

    /// A 302 is not followed: `Api` with status 302, the target is never hit.
    #[tokio::test]
    async fn redirect_is_not_followed() {
        use crate::provider::test_stub::{StubBody, StubReply, StubServer};
        let b = StubServer::start(vec![(
            ("POST", "/v1/messages"),
            (
                200,
                "text/event-stream",
                include_str!("../../tests/fixtures/anthropic_text.sse").to_string(),
            ),
        )])
        .await;
        let a = StubServer::start_replies(vec![(
            ("POST", "/v1/messages"),
            vec![StubReply {
                status: 302,
                content_type: "text/plain",
                headers: vec![("location", format!("{}/v1/messages", b.url()))],
                body: StubBody::Full(String::new()),
            }],
        )])
        .await;
        let provider = AnthropicApi::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("sk-test".to_string()),
            &a.url(),
        );
        let err = provider.complete(hi_request()).await.expect_err("302");
        assert_eq!(err.kind(), crate::provider::error::ErrorKind::Other);
        match err {
            Error::Api(e) => assert_eq!(e.status, Some(302)),
            other => panic!("expected Api, got {other:?}"),
        }
        assert_eq!(a.requests().len(), 1);
        assert_eq!(b.requests().len(), 0);
    }

    /// A 200 KiB error body is cut to the 64 KiB cap.
    #[tokio::test]
    async fn error_body_is_capped() {
        let stub = crate::provider::test_stub::StubServer::start(vec![(
            ("POST", "/v1/messages"),
            (500, "text/plain", "a".repeat(200 * 1024)),
        )])
        .await;
        let provider = AnthropicApi::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("sk-test".to_string()),
            &stub.url(),
        );
        match provider.complete(hi_request()).await.expect_err("500") {
            Error::Api(e) => {
                assert_eq!(e.status, Some(500));
                assert_eq!(e.message, "a".repeat(65_536));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    /// An error body that never arrives ends in `IdleTimeout`.
    #[tokio::test]
    async fn error_body_idle_timeout() {
        use crate::provider::test_stub::{StubBody, StubReply, StubServer};
        let stub = StubServer::start_replies(vec![(
            ("POST", "/v1/messages"),
            vec![StubReply {
                status: 500,
                content_type: "application/json",
                headers: vec![],
                body: StubBody::HeadersThenHang,
            }],
        )])
        .await;
        let provider = AnthropicApi::with_options(
            "m",
            AuthMethod::ApiKey("sk-test".to_string()),
            &stub.url(),
            ProviderOptions {
                connect_timeout: Duration::from_secs(30),
                idle_gap: Duration::from_secs(1),
            },
        )
        .expect("provider");
        let started = std::time::Instant::now();
        let err = provider.complete(hi_request()).await.expect_err("hang");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            matches!(err, Error::IdleTimeout { after } if after == Duration::from_secs(1)),
            "got {err:?}"
        );
    }

    /// A 2 MiB line without newline fails with the line cap.
    #[test]
    fn line_cap() {
        let mut acc = LineAccumulator::new();
        acc.extend(&vec![b'x'; 2 * 1024 * 1024]);
        match acc.next_line() {
            Err(Error::SseParse { message }) => {
                assert_eq!(message, "SSE line exceeds 1 MiB")
            }
            other => panic!("expected SseParse, got {other:?}"),
        }
    }
}
