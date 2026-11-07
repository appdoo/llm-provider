//! SSE stream parser for Anthropic's streaming format.

use crate::provider::{StopReason, StreamEvent};

use crate::provider::error::{ApiError, ProviderError};

/// Map an Anthropic `stop_reason` to a `StopReason`. Shared by streaming
/// and batch decoding.
pub(crate) fn anthropic_stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::Refusal,
        "pause_turn" => StopReason::PauseTurn,
        _ => StopReason::Unknown,
    }
}

/// Largest `data:` payload of one SSE event.
const MAX_EVENT_BYTES: usize = 1024 * 1024;
/// Largest streamed input of one tool call.
const MAX_TOOL_INPUT_BYTES: usize = 1024 * 1024;

fn too_big(message: &str) -> ProviderError {
    ProviderError::SseParse {
        message: message.to_string(),
    }
}

/// Stateful SSE parser for Anthropic's line-based SSE format.
///
/// Feed lines one at a time. An empty line marks the end of an event.
pub(crate) struct SseParser {
    event_type: Option<String>,
    data_buffer: String,
    // Tool input accumulation: content_block_start gives id+name,
    // input_json_delta fragments build up the input JSON,
    // content_block_stop emits the complete ToolUse.
    pending_tool_id: Option<String>,
    pending_tool_name: Option<String>,
    pending_tool_input: String,
    /// `message_delta.delta.stop_reason`, handed out by `take_stop`.
    pending_stop: Option<StopReason>,
}

impl SseParser {
    pub(crate) fn new() -> Self {
        Self {
            event_type: None,
            data_buffer: String::new(),
            pending_tool_id: None,
            pending_tool_name: None,
            pending_tool_input: String::new(),
            pending_stop: None,
        }
    }

    /// The stop reason seen in `message_delta`, if any. Taken once.
    pub(crate) fn take_stop(&mut self) -> Option<StopReason> {
        self.pending_stop.take()
    }

    /// Feed a single line from the SSE stream.
    ///
    /// Returns `Some(event)` when a complete SSE event has been parsed.
    pub(crate) fn feed_line(&mut self, line: &str) -> Result<Option<StreamEvent>, ProviderError> {
        if line.is_empty() {
            // Empty line = event boundary
            if self.data_buffer.is_empty() {
                return Ok(None);
            }
            let event = self.parse_event()?;
            self.event_type = None;
            self.data_buffer.clear();
            return Ok(event);
        }

        if let Some(value) = line.strip_prefix("event: ") {
            self.event_type = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("data: ") {
            self.data_buffer.push_str(value);
            if self.data_buffer.len() > MAX_EVENT_BYTES {
                return Err(too_big("SSE event exceeds 1 MiB"));
            }
        }

        Ok(None)
    }

    fn parse_event(&mut self) -> Result<Option<StreamEvent>, ProviderError> {
        let data: serde_json::Value =
            serde_json::from_str(&self.data_buffer).map_err(|e| ProviderError::SseParse {
                message: format!("Invalid JSON: {}", e),
            })?;

        let event_type = self.event_type.as_deref().unwrap_or("");

        match event_type {
            "content_block_delta" => {
                let delta = &data["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => {
                        let text = delta["text"].as_str().unwrap_or("").to_string();
                        Ok(Some(StreamEvent::ContentDelta(text)))
                    }
                    Some("input_json_delta") => {
                        // Accumulate tool input JSON fragments
                        if let Some(partial) = delta["partial_json"].as_str() {
                            self.pending_tool_input.push_str(partial);
                            if self.pending_tool_input.len() > MAX_TOOL_INPUT_BYTES {
                                return Err(too_big("tool input exceeds 1 MiB"));
                            }
                        }
                        Ok(None)
                    }
                    _ => Ok(None),
                }
            }
            "content_block_start" => {
                let content_block = &data["content_block"];
                if content_block["type"].as_str() == Some("tool_use") {
                    // Buffer tool id+name; emit ToolUse at content_block_stop
                    self.pending_tool_id =
                        Some(content_block["id"].as_str().unwrap_or("").to_string());
                    self.pending_tool_name =
                        Some(content_block["name"].as_str().unwrap_or("").to_string());
                    self.pending_tool_input.clear();
                }
                Ok(None)
            }
            "message_stop" => Ok(Some(StreamEvent::MessageStop)),
            "message_delta" => {
                if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                    self.pending_stop = Some(anthropic_stop_reason(reason));
                }
                if let Some(usage) = data.get("usage") {
                    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0) as u32;
                    Ok(Some(StreamEvent::Usage {
                        input_tokens: 0,
                        output_tokens,
                        cache_creation_input_tokens: 0,
                        cache_read_input_tokens: 0,
                    }))
                } else {
                    Ok(None)
                }
            }
            "message_start" => {
                if let Some(usage) = data["message"].get("usage") {
                    // `message_start.usage.output_tokens` is deliberately
                    // ignored: `message_delta` carries the cumulative total.
                    let read = |key: &str| usage[key].as_u64().unwrap_or(0) as u32;
                    Ok(Some(StreamEvent::Usage {
                        input_tokens: read("input_tokens"),
                        output_tokens: 0,
                        cache_creation_input_tokens: read("cache_creation_input_tokens"),
                        cache_read_input_tokens: read("cache_read_input_tokens"),
                    }))
                } else {
                    Ok(None)
                }
            }
            "ping" => Ok(None),
            "content_block_stop" => {
                // If we have a pending tool use, emit it now with accumulated input.
                //
                // A malformed `input_json_delta` stream is surfaced as
                // `ProviderError::SseParse` with tool name + id for debugging.
                if let (Some(id), Some(name)) =
                    (self.pending_tool_id.take(), self.pending_tool_name.take())
                {
                    let input = if self.pending_tool_input.is_empty() {
                        serde_json::Value::Object(serde_json::Map::new())
                    } else {
                        match serde_json::from_str::<serde_json::Value>(&self.pending_tool_input) {
                            Ok(v) => v,
                            Err(e) => {
                                // Clear accumulator before returning so a
                                // reused parser does not carry the failed
                                // fragment into subsequent events.
                                self.pending_tool_input.clear();
                                return Err(ProviderError::SseParse {
                                    message: format!(
                                        "tool_use input parse error (tool {name}, id {id}): {e}"
                                    ),
                                });
                            }
                        }
                    };
                    self.pending_tool_input.clear();
                    Ok(Some(StreamEvent::ToolUse { id, name, input }))
                } else {
                    Ok(None)
                }
            }
            "error" => Err(ProviderError::Api(ApiError::from_stream_event(&data, true))),
            _ => Ok(None),
        }
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_content_block_delta_text() {
        let mut parser = SseParser::new();
        assert!(parser
            .feed_line("event: content_block_delta")
            .unwrap()
            .is_none());
        assert!(parser
            .feed_line(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#)
            .unwrap()
            .is_none());

        let event = parser.feed_line("").unwrap();
        match event {
            Some(StreamEvent::ContentDelta(text)) => assert_eq!(text, "Hi"),
            other => panic!("expected ContentDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_message_stop() {
        let mut parser = SseParser::new();
        parser.feed_line("event: message_stop").unwrap();
        parser
            .feed_line(r#"data: {"type":"message_stop"}"#)
            .unwrap();

        let event = parser.feed_line("").unwrap();
        match event {
            Some(StreamEvent::MessageStop) => {}
            other => panic!("expected MessageStop, got {:?}", other),
        }
    }

    #[test]
    fn parse_tool_use_with_input() {
        let mut parser = SseParser::new();

        // content_block_start: buffers tool id+name
        parser.feed_line("event: content_block_start").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"bash"}}"#)
            .unwrap();
        let event = parser.feed_line("").unwrap();
        assert!(event.is_none(), "ToolUse should not emit at start");

        // input_json_delta: accumulate input fragments
        parser.feed_line("event: content_block_delta").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\""}}"#)
            .unwrap();
        assert!(parser.feed_line("").unwrap().is_none());

        parser.feed_line("event: content_block_delta").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":": \"ls\"}"}}"#)
            .unwrap();
        assert!(parser.feed_line("").unwrap().is_none());

        // content_block_stop: emit complete ToolUse with parsed input
        parser.feed_line("event: content_block_stop").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_stop","index":1}"#)
            .unwrap();
        let event = parser.feed_line("").unwrap();
        match event {
            Some(StreamEvent::ToolUse { id, name, input }) => {
                assert_eq!(id, "toolu_123");
                assert_eq!(name, "bash");
                assert_eq!(input["command"], "ls");
            }
            other => panic!("expected ToolUse with input, got {:?}", other),
        }
    }

    #[test]
    fn parse_multiple_events() {
        let mut parser = SseParser::new();
        let lines = [
            "event: content_block_delta",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
            "",
            "event: content_block_delta",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}"#,
            "",
            "event: message_stop",
            r#"data: {"type":"message_stop"}"#,
            "",
        ];

        let mut events = Vec::new();
        for line in &lines {
            if let Some(event) = parser.feed_line(line).unwrap() {
                events.push(event);
            }
        }

        assert_eq!(events.len(), 3);
        match &events[0] {
            StreamEvent::ContentDelta(t) => assert_eq!(t, "Hello"),
            other => panic!("expected ContentDelta, got {:?}", other),
        }
        match &events[1] {
            StreamEvent::ContentDelta(t) => assert_eq!(t, " world"),
            other => panic!("expected ContentDelta, got {:?}", other),
        }
        assert!(matches!(&events[2], StreamEvent::MessageStop));
    }

    #[test]
    fn partial_line_does_not_yield() {
        let mut parser = SseParser::new();
        // Feed event and data lines but no blank line to terminate
        assert!(parser
            .feed_line("event: content_block_delta")
            .unwrap()
            .is_none());
        assert!(parser
            .feed_line(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}"#)
            .unwrap()
            .is_none());
        // No blank line fed — no event should be emitted
    }

    #[test]
    fn parse_message_start_usage() {
        let mut parser = SseParser::new();
        parser.feed_line("event: message_start").unwrap();
        parser
            .feed_line(r#"data: {"type":"message_start","message":{"usage":{"input_tokens":42}}}"#)
            .unwrap();

        let event = parser.feed_line("").unwrap();
        match event {
            Some(StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            }) => {
                assert_eq!(input_tokens, 42);
                assert_eq!(output_tokens, 0);
            }
            other => panic!("expected Usage, got {:?}", other),
        }
    }

    #[test]
    fn parse_message_delta_usage() {
        let mut parser = SseParser::new();
        parser.feed_line("event: message_delta").unwrap();
        parser
            .feed_line(r#"data: {"type":"message_delta","usage":{"output_tokens":100}}"#)
            .unwrap();

        let event = parser.feed_line("").unwrap();
        match event {
            Some(StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            }) => {
                assert_eq!(input_tokens, 0);
                assert_eq!(output_tokens, 100);
            }
            other => panic!("expected Usage, got {:?}", other),
        }
    }

    /// `message_start` carries both cache counters from
    /// `message.usage`; `output_tokens` is reported as 0.
    #[test]
    fn message_start_reports_cache_counters() {
        let mut parser = SseParser::new();
        assert!(parser.feed_line("event: message_start").unwrap().is_none());
        assert!(parser
            .feed_line(r#"data: {"type":"message_start","message":{"usage":{"input_tokens":5,"cache_creation_input_tokens":1200,"cache_read_input_tokens":0}}}"#)
            .unwrap()
            .is_none());

        let event = parser.feed_line("").unwrap();
        match event {
            Some(StreamEvent::Usage {
                input_tokens,
                output_tokens,
                cache_creation_input_tokens,
                cache_read_input_tokens,
            }) => {
                assert_eq!(input_tokens, 5);
                assert_eq!(output_tokens, 0);
                assert_eq!(cache_creation_input_tokens, 1200);
                assert_eq!(cache_read_input_tokens, 0);
            }
            other => panic!("expected Usage, got {:?}", other),
        }
    }

    #[test]
    fn parse_error_event() {
        let mut parser = SseParser::new();
        parser.feed_line("event: error").unwrap();
        parser
            .feed_line(r#"data: {"type":"error","error":{"message":"rate limited"}}"#)
            .unwrap();

        match parser.feed_line("") {
            Err(ProviderError::Api(e)) => {
                assert_eq!(e.message, "rate limited");
                assert_eq!(e.status, None);
            }
            other => panic!("expected Err(Api), got {:?}", other),
        }
    }

    /// `message_delta` stores the stop reason and still emits Usage.
    #[test]
    fn message_delta_stores_stop_reason() {
        let mut parser = SseParser::new();
        parser.feed_line("event: message_delta").unwrap();
        parser
            .feed_line(r#"data: {"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":5}}"#)
            .unwrap();
        assert!(matches!(
            parser.feed_line("").unwrap(),
            Some(StreamEvent::Usage {
                output_tokens: 5,
                ..
            })
        ));
        assert_eq!(parser.take_stop(), Some(StopReason::MaxTokens));
        assert_eq!(parser.take_stop(), None);
    }

    /// The Anthropic stop table.
    #[test]
    fn anthropic_stop_reason_table() {
        for (wire, want) in [
            ("end_turn", StopReason::EndTurn),
            ("tool_use", StopReason::ToolUse),
            ("max_tokens", StopReason::MaxTokens),
            ("stop_sequence", StopReason::StopSequence),
            ("refusal", StopReason::Refusal),
            ("pause_turn", StopReason::PauseTurn),
            ("something_new", StopReason::Unknown),
        ] {
            assert_eq!(anthropic_stop_reason(wire), want, "{wire}");
        }
    }

    #[test]
    fn ping_event_is_ignored() {
        let mut parser = SseParser::new();
        parser.feed_line("event: ping").unwrap();
        parser.feed_line(r#"data: {"type":"ping"}"#).unwrap();

        let event = parser.feed_line("").unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn empty_lines_without_data_are_ignored() {
        let mut parser = SseParser::new();
        // Multiple blank lines should not produce events
        assert!(parser.feed_line("").unwrap().is_none());
        assert!(parser.feed_line("").unwrap().is_none());
    }

    // --- tool_use input parse error propagation ---

    /// When `input_json_delta` accumulates to an unterminated JSON
    /// fragment (e.g. `{"command"` with no closing `}`), the
    /// `content_block_stop` event must surface a `ProviderError::SseParse`
    /// naming the tool and id, instead of silently emitting a `ToolUse`
    /// with an empty input object. Masking malformed tool input as `{}`
    /// caused downstream tools to execute against the wrong arguments.
    #[test]
    fn toolu_use_input_parse_error_propagates() {
        let mut parser = SseParser::new();

        // content_block_start: buffer tool id + name.
        parser.feed_line("event: content_block_start").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_bad","name":"bash"}}"#)
            .unwrap();
        assert!(parser.feed_line("").unwrap().is_none());

        // input_json_delta: an unterminated JSON fragment.
        parser.feed_line("event: content_block_delta").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\""}}"#)
            .unwrap();
        assert!(parser.feed_line("").unwrap().is_none());

        // content_block_stop: must now surface a Stream parse error.
        parser.feed_line("event: content_block_stop").unwrap();
        parser
            .feed_line(r#"data: {"type":"content_block_stop","index":0}"#)
            .unwrap();

        match parser.feed_line("") {
            Err(ProviderError::SseParse { message: msg }) => {
                assert!(
                    msg.contains("tool_use input parse error"),
                    "message must name the failure mode, got: {msg}"
                );
                assert!(
                    msg.contains("toolu_bad"),
                    "message must include the tool id, got: {msg}"
                );
                assert!(
                    msg.contains("bash"),
                    "message must include the tool name, got: {msg}"
                );
            }
            other => panic!(
                "expected ProviderError::SseParse with parse details, got {:?}",
                other
            ),
        }
    }

    fn feed_event(
        parser: &mut SseParser,
        event: &str,
        data: &str,
    ) -> Result<Option<StreamEvent>, ProviderError> {
        parser.feed_line(&format!("event: {event}"))?;
        parser.feed_line(&format!("data: {data}"))?;
        parser.feed_line("")
    }

    /// Tool input beyond 1 MiB fails on the 1 025th delta, no ToolUse before.
    #[test]
    fn tool_input_cap() {
        let mut parser = SseParser::new();
        let start = r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"Write"}}"#;
        assert!(feed_event(&mut parser, "content_block_start", start)
            .unwrap()
            .is_none());
        let delta = format!(
            r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"input_json_delta","partial_json":"{}"}}}}"#,
            "a".repeat(1024)
        );
        for n in 1..=1025 {
            let r = feed_event(&mut parser, "content_block_delta", &delta);
            if n <= 1024 {
                assert!(matches!(r, Ok(None)), "delta {n}: {r:?}");
            } else {
                match r {
                    Err(ProviderError::SseParse { message }) => {
                        assert_eq!(message, "tool input exceeds 1 MiB")
                    }
                    other => panic!("expected SseParse, got {other:?}"),
                }
            }
        }
    }

    /// Joined `data:` lines beyond 1 MiB fail on the 1 025th line.
    #[test]
    fn event_cap() {
        let mut parser = SseParser::new();
        parser.feed_line("event: content_block_delta").unwrap();
        let line = format!("data: {}", "x".repeat(1024));
        for n in 1..=1025 {
            let r = parser.feed_line(&line);
            if n <= 1024 {
                assert!(matches!(r, Ok(None)), "line {n}: {r:?}");
            } else {
                match r {
                    Err(ProviderError::SseParse { message }) => {
                        assert_eq!(message, "SSE event exceeds 1 MiB")
                    }
                    other => panic!("expected SseParse, got {other:?}"),
                }
            }
        }
    }
}
