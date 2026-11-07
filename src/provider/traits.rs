//! LlmProvider trait and request/response types.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_stream::{Stream, StreamExt};

use crate::provider::StreamEvent;

use crate::provider::error::Error;

/// Network timeouts of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderOptions {
    /// Limit for establishing the TCP/TLS connection.
    pub connect_timeout: Duration,
    /// Longest silence allowed before the response headers and between
    /// body chunks.
    pub idle_gap: Duration,
}

impl Default for ProviderOptions {
    /// Connect 30 s, idle gap 90 s.
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            idle_gap: Duration::from_secs(90),
        }
    }
}

/// Selection of the model to use for a completion request.
///
/// `Default` asks the provider to resolve the model from its own
/// configuration (e.g. the model passed to `AnthropicApi::with_auth`).
/// `Named(id)` overrides that default with an explicit model id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSelection {
    /// Use the provider's configured default model.
    Default,
    /// Override with a specific model id.
    Named(String),
}

/// A request to complete a conversation.
///
/// `messages` is `Arc<[Message]>` so callers that retain their own history
/// can clone the request cheaply without duplicating the message buffer.
/// Serde serializes `Arc<[T]>` as a JSON array — byte-identical to `Vec<T>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Which model to use. `ModelSelection::Default` asks the provider
    /// to use its own configured default; `ModelSelection::Named(id)`
    /// overrides.
    pub model: ModelSelection,
    pub system: Option<String>,
    pub messages: Arc<[Message]>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    /// Prompt-cache breakpoints. Empty means no caching; an empty list is
    /// omitted from serialized JSON so pre-existing payloads stay unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_breakpoints: Vec<CacheBreakpoint>,
}

/// A prompt-cache breakpoint: cache the prefix up to and including
/// `position` for `ttl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheBreakpoint {
    pub position: BreakpointPosition,
    pub ttl: CacheTtl,
}

/// Where in the request a cache breakpoint sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointPosition {
    /// After the last tool definition.
    Tools,
    /// After the system prompt.
    System,
    /// After the last content block of `messages[i]`.
    Message(usize),
}

/// Lifetime of a prompt-cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CacheTtl {
    FiveMinutes,
    OneHour,
}

/// Token usage of one completion, summed over all usage events.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,
}

use super::event::StopReason;

/// A tool call collected from a completion.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolUseCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// The collected result of a one-shot completion.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub text: String,
    pub tool_uses: Vec<ToolUseCall>,
    pub usage: Usage,
    pub stop: StopReason,
}

/// A single message in the conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: Vec<Content>,
}

/// The role of a message sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// Content within a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Content {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// Definition of a tool the model can use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Metadata about a provider.
#[derive(Debug)]
pub struct ProviderInfo {
    pub name: String,
    pub model: String,
    pub supports_tools: bool,
    pub supports_streaming: bool,
}

/// Trait for LLM providers that can stream completions.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a completion. Returns events as they arrive.
    fn stream(
        &self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;

    /// Provider metadata.
    fn info(&self) -> ProviderInfo;

    /// One-shot completion: streams the request once and folds the events
    /// into a `Completion`. Any error aborts the fold; no partial result.
    async fn complete(&self, request: CompletionRequest) -> Result<Completion, Error> {
        let mut events = self.stream(request);
        let mut text = String::new();
        let mut tool_uses = Vec::new();
        let mut usage = Usage::default();
        let mut stop_seen: Option<StopReason> = None;

        while let Some(event) = events.next().await {
            match event? {
                StreamEvent::ContentDelta(delta) => text.push_str(&delta),
                StreamEvent::ToolUse { id, name, input } => {
                    tool_uses.push(ToolUseCall { id, name, input })
                }
                StreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                    cache_creation_input_tokens,
                    cache_read_input_tokens,
                } => {
                    usage.input_tokens = usage.input_tokens.saturating_add(input_tokens);
                    usage.output_tokens = usage.output_tokens.saturating_add(output_tokens);
                    usage.cache_creation_input_tokens = usage
                        .cache_creation_input_tokens
                        .saturating_add(cache_creation_input_tokens);
                    usage.cache_read_input_tokens = usage
                        .cache_read_input_tokens
                        .saturating_add(cache_read_input_tokens);
                }
                StreamEvent::Stop(reason) => stop_seen = Some(reason),
                StreamEvent::MessageStop => {
                    // Without a provider stop reason, infer it from tool uses.
                    let stop = stop_seen.unwrap_or(if tool_uses.is_empty() {
                        StopReason::EndTurn
                    } else {
                        StopReason::ToolUse
                    });
                    return Ok(Completion {
                        text,
                        tool_uses,
                        usage,
                        stop,
                    });
                }
            }
        }
        Err(Error::SseParse {
            message: "stream ended before message_stop".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_messages() -> Arc<[Message]> {
        Arc::from(vec![Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: "hello".to_string(),
            }],
        }])
    }

    /// `CompletionRequest` constructs with `Arc<[Message]>`; cloning
    /// shares the backing slice (Arc::ptr_eq on inner).
    #[test]
    fn completion_request_arc_messages_shares_buffer_on_clone() {
        let req = CompletionRequest {
            model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
            system: None,
            messages: sample_messages(),
            tools: vec![],
            max_tokens: 1024,
            cache_breakpoints: Vec::new(),
        };
        let cloned = req.clone();
        assert!(
            Arc::ptr_eq(&req.messages, &cloned.messages),
            "cloned CompletionRequest must share the message buffer via Arc"
        );
        assert_eq!(req.messages.len(), 1);
    }

    /// JSON round-trip: `Arc<[Message]>` serializes identically to
    /// `Vec<Message>` (both produce a JSON array), and the deserialized value
    /// matches the recorded golden snapshot.
    #[test]
    fn completion_request_json_roundtrip_matches_golden() {
        let req = CompletionRequest {
            model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
            system: Some("You are helpful.".to_string()),
            messages: Arc::from(vec![Message {
                role: MessageRole::User,
                content: vec![Content::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"cmd": "ls"}),
                }],
            }]),
            tools: vec![],
            max_tokens: 1024,
            cache_breakpoints: Vec::new(),
        };

        // Golden snapshot — updated for
        // `ModelSelection` (newtype `Named(String)` serde-encodes as
        // `{"named":"..."}` with the default externally-tagged
        // representation). The `messages` array shape is untouched —
        // what this test originally pinned remains pinned.
        let golden = r#"{"model":{"named":"claude-sonnet-4-6"},"system":"You are helpful.","messages":[{"role":"user","content":[{"type":"tool_use","id":"toolu_1","name":"bash","input":{"cmd":"ls"}}]}],"tools":[],"max_tokens":1024}"#;

        let encoded =
            serde_json::to_string(&req).expect("CompletionRequest must serialize to JSON");
        assert_eq!(
            encoded, golden,
            "serialized JSON must match golden snapshot"
        );

        // Round-trip: deserialize back and re-serialize to confirm stability.
        let decoded: CompletionRequest =
            serde_json::from_str(&encoded).expect("JSON must deserialize back");
        let re_encoded = serde_json::to_string(&decoded).expect("re-serialize");
        assert_eq!(re_encoded, golden, "round-trip must be byte-identical");

        // Also verify that an empty message slice serialises as `[]` — the
        // base invariant of `Arc<[T]> == Vec<T>` in JSON array shape.
        let empty = CompletionRequest {
            model: ModelSelection::Named("m".to_string()),
            system: None,
            messages: Arc::from(Vec::<Message>::new()),
            tools: vec![],
            max_tokens: 1,
            cache_breakpoints: Vec::new(),
        };
        let empty_json = serde_json::to_string(&empty).expect("empty request must serialise");
        assert!(
            empty_json.contains(r#""messages":[]"#),
            "empty messages slice must render as '[]', got: {}",
            empty_json
        );
    }

    // --- complete() ---

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;

    /// Mock whose `stream()` yields the events built by `events`.
    struct ScriptedProvider {
        events: fn() -> Vec<Result<StreamEvent, Error>>,
    }

    #[async_trait]
    impl LlmProvider for ScriptedProvider {
        fn stream(&self, _request: CompletionRequest) -> EventStream {
            Box::pin(tokio_stream::iter((self.events)()))
        }
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "scripted".to_string(),
                model: "m".to_string(),
                supports_tools: false,
                supports_streaming: true,
            }
        }
    }

    fn usage_event(input_tokens: u32, output_tokens: u32) -> StreamEvent {
        StreamEvent::Usage {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        }
    }

    fn request() -> CompletionRequest {
        CompletionRequest {
            model: ModelSelection::Default,
            system: None,
            messages: sample_messages(),
            tools: vec![],
            max_tokens: 16,
            cache_breakpoints: Vec::new(),
        }
    }

    /// Text deltas concatenate, usage sums, stop is EndTurn.
    #[tokio::test]
    async fn complete_collects_text_and_usage() {
        let provider = ScriptedProvider {
            events: || {
                vec![
                    Ok(StreamEvent::ContentDelta("Hel".to_string())),
                    Ok(StreamEvent::ContentDelta("lo".to_string())),
                    Ok(usage_event(12, 2)),
                    Ok(StreamEvent::MessageStop),
                ]
            },
        };
        let completion = provider.complete(request()).await.expect("complete");
        assert_eq!(
            completion,
            Completion {
                text: "Hello".to_string(),
                tool_uses: vec![],
                usage: Usage {
                    input_tokens: 12,
                    output_tokens: 2,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
                stop: StopReason::EndTurn,
            }
        );
    }

    fn overloaded() -> crate::provider::error::ApiError {
        crate::provider::error::ApiError {
            kind: crate::provider::error::ErrorKind::Overloaded,
            status: None,
            message: "overloaded".to_string(),
            request_id: None,
            retry_after: None,
        }
    }

    /// Without `Stop`, the reason is inferred from tool uses;
    /// with `Stop`, the last one wins.
    #[tokio::test]
    async fn complete_infers_stop_without_stop_event() {
        let tool = ScriptedProvider {
            events: || {
                vec![
                    Ok(StreamEvent::ToolUse {
                        id: "t".to_string(),
                        name: "n".to_string(),
                        input: serde_json::json!({}),
                    }),
                    Ok(StreamEvent::MessageStop),
                ]
            },
        };
        assert_eq!(
            tool.complete(request()).await.expect("complete").stop,
            StopReason::ToolUse
        );
        let text = ScriptedProvider {
            events: || {
                vec![
                    Ok(StreamEvent::ContentDelta("x".to_string())),
                    Ok(StreamEvent::MessageStop),
                ]
            },
        };
        assert_eq!(
            text.complete(request()).await.expect("complete").stop,
            StopReason::EndTurn
        );
        let stopped = ScriptedProvider {
            events: || {
                vec![
                    Ok(StreamEvent::ContentDelta("x".to_string())),
                    Ok(StreamEvent::Stop(StopReason::MaxTokens)),
                    Ok(StreamEvent::MessageStop),
                ]
            },
        };
        assert_eq!(
            stopped.complete(request()).await.expect("complete").stop,
            StopReason::MaxTokens
        );
    }

    /// The first stream error is returned unchanged.
    #[tokio::test]
    async fn complete_propagates_stream_error() {
        let provider = ScriptedProvider {
            events: || {
                vec![
                    Ok(StreamEvent::ContentDelta("Hi".to_string())),
                    Err(Error::Api(overloaded())),
                ]
            },
        };
        match provider.complete(request()).await {
            Err(Error::Api(e)) => assert_eq!(e, overloaded()),
            other => panic!("expected Api(overloaded), got {:?}", other),
        }
    }

    /// A stream that ends without MessageStop is an error.
    #[tokio::test]
    async fn complete_without_message_stop_is_error() {
        let provider = ScriptedProvider {
            events: || vec![Ok(StreamEvent::ContentDelta("Hi".to_string()))],
        };
        match provider.complete(request()).await {
            Err(Error::SseParse { message }) => {
                assert_eq!(message, "stream ended before message_stop")
            }
            other => panic!("expected SseParse error, got {:?}", other),
        }
    }

    /// Mock that counts `stream()` calls and captures the request.
    struct RecordingProvider {
        calls: AtomicUsize,
        captured: Mutex<Option<CompletionRequest>>,
    }

    #[async_trait]
    impl LlmProvider for RecordingProvider {
        fn stream(&self, request: CompletionRequest) -> EventStream {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.captured.lock().expect("mutex poisoned") = Some(request);
            Box::pin(tokio_stream::iter(vec![Ok(StreamEvent::MessageStop)]))
        }
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "recording".to_string(),
                model: "m".to_string(),
                supports_tools: false,
                supports_streaming: true,
            }
        }
    }

    /// A text message serializes to the exact wire shape and round-trips.
    #[test]
    fn text_message_serializes_to_wire_shape() {
        let msg = Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: "Hello".into(),
            }],
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert_eq!(
            json,
            r#"{"role":"user","content":[{"type":"text","text":"Hello"}]}"#
        );
        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, msg);
    }

    /// Text, tool_use and tool_result survive a JSON round-trip.
    #[test]
    fn mixed_content_message_round_trips() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: vec![
                Content::Text { text: "a".into() },
                Content::ToolUse {
                    id: "toolu_1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({"path": "a.txt"}),
                },
                Content::ToolResult {
                    tool_use_id: "toolu_1".into(),
                    content: "ok".into(),
                    is_error: false,
                },
            ],
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, msg);
    }

    /// Complete() streams exactly once.
    #[tokio::test]
    async fn complete_calls_stream_once() {
        let provider = RecordingProvider {
            calls: AtomicUsize::new(0),
            captured: Mutex::new(None),
        };
        provider.complete(request()).await.expect("complete");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}
