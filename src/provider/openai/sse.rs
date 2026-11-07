//! Decoder for OpenAI-style `/chat/completions` streams: `data: {chunk}`
//! lines, tool-call fragments buffered per slot (by `index`, else by `id`,
//! else the last slot), `data: [DONE]` at the end.

use std::collections::HashMap;

use serde_json::Value;
use tokio_stream::{Stream, StreamExt};

use crate::provider::{StopReason, StreamEvent};

use crate::provider::error::{ApiError, Error, Oversize};
use crate::transport::LineAccumulator;

/// Map an OpenAI `finish_reason` to a `StopReason`.
fn openai_stop_reason(s: &str) -> StopReason {
    match s {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::Refusal,
        _ => StopReason::Unknown,
    }
}

/// Largest streamed input of one tool call.
const MAX_TOOL_INPUT_BYTES: usize = 1024 * 1024;

/// One tool call assembled from its streamed fragments.
#[derive(Default)]
struct PartialCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// Line-level state machine: turns `data:` lines into `StreamEvent`s.
#[derive(Default)]
struct ChunkDecoder {
    /// Tool-call slots in creation order.
    calls: Vec<PartialCall>,
    /// `index` → slot, for fragments that carry an index.
    by_index: HashMap<u64, usize>,
    /// `id` → slot, for index-less continuation fragments.
    by_id: HashMap<String, usize>,
    /// The slot the previous fragment used.
    last: Option<usize>,
    flushed: bool,
    /// The first non-null `finish_reason`, emitted as `Stop` at `[DONE]`.
    stop: Option<StopReason>,
}

impl ChunkDecoder {
    /// Decode one line. Returns the events it produces and whether the
    /// stream is finished (`[DONE]`). An `Err` ends the stream.
    fn feed_line(&mut self, line: &str) -> Result<(Vec<StreamEvent>, bool), Error> {
        let Some(data) = line.strip_prefix("data: ") else {
            return Ok((Vec::new(), false));
        };
        let mut events = Vec::new();
        if data.trim() == "[DONE]" {
            self.flush(&mut events)?;
            if let Some(reason) = self.stop {
                events.push(StreamEvent::Stop(reason));
            }
            events.push(StreamEvent::MessageStop);
            return Ok((events, true));
        }

        let chunk: Value = serde_json::from_str(data)?;
        if chunk.get("error").is_some() {
            return Err(Error::Api(ApiError::from_openai_event(&chunk)));
        }

        if let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            let delta = &choice["delta"];
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    events.push(StreamEvent::ContentDelta(text.to_string()));
                }
            }
            if let Some(fragments) = delta.get("tool_calls").and_then(Value::as_array) {
                for fragment in fragments {
                    self.buffer_fragment(fragment)?;
                }
            }
            if let Some(reason) = choice.get("finish_reason").filter(|r| !r.is_null()) {
                if self.stop.is_none() {
                    self.stop = Some(
                        reason
                            .as_str()
                            .map_or(StopReason::Unknown, openai_stop_reason),
                    );
                }
                self.flush(&mut events)?;
            }
        }

        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            let to_u32 = |v: Option<&Value>| {
                v.and_then(Value::as_u64)
                    .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
            };
            let cached = to_u32(
                usage
                    .get("prompt_tokens_details")
                    .and_then(|d| d.get("cached_tokens")),
            );
            events.push(StreamEvent::Usage {
                input_tokens: to_u32(usage.get("prompt_tokens")).saturating_sub(cached),
                output_tokens: to_u32(usage.get("completion_tokens")),
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: cached,
            });
        }
        Ok((events, false))
    }

    /// Pick the fragment's slot: by `index`, else by known `id`, else a new
    /// slot for a new `id`, else the last slot (or a new one).
    fn slot_for(&mut self, index: Option<u64>, id: Option<&str>) -> usize {
        let existing = match (index, id) {
            (Some(i), _) => self.by_index.get(&i).copied(),
            (None, Some(id)) => self.by_id.get(id).copied(),
            (None, None) => self.last,
        };
        existing.unwrap_or_else(|| {
            self.calls.push(PartialCall::default());
            let slot = self.calls.len() - 1;
            if let Some(i) = index {
                self.by_index.insert(i, slot);
            }
            slot
        })
    }

    fn buffer_fragment(&mut self, fragment: &Value) -> Result<(), Error> {
        let index = fragment.get("index").and_then(Value::as_u64);
        let id = fragment.get("id").and_then(Value::as_str);
        let slot = self.slot_for(index, id);
        self.last = Some(slot);
        let call = &mut self.calls[slot];
        if call.id.is_none() {
            if let Some(id) = id {
                call.id = Some(id.to_string());
                self.by_id.insert(id.to_string(), slot);
            }
        }
        let function = &fragment["function"];
        if call.name.is_none() {
            call.name = function
                .get("name")
                .and_then(Value::as_str)
                .map(String::from);
        }
        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
            call.arguments.push_str(arguments);
            if call.arguments.len() > MAX_TOOL_INPUT_BYTES {
                return Err(Error::TooLarge {
                    what: Oversize::ToolInput,
                    max: MAX_TOOL_INPUT_BYTES,
                });
            }
        }
        Ok(())
    }

    /// Emit every buffered call as one `ToolUse`. Runs once. Index order
    /// when every slot came from an indexed fragment, else creation order.
    fn flush(&mut self, events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        let mut calls: Vec<Option<PartialCall>> = std::mem::take(&mut self.calls)
            .into_iter()
            .map(Some)
            .collect();
        let order: Vec<usize> = if self.by_index.len() == calls.len() {
            let mut indexed: Vec<(u64, usize)> =
                self.by_index.iter().map(|(&i, &slot)| (i, slot)).collect();
            indexed.sort_unstable();
            indexed.into_iter().map(|(_, slot)| slot).collect()
        } else {
            (0..calls.len()).collect()
        };
        for call in order.into_iter().filter_map(|slot| calls[slot].take()) {
            let input: Value = if call.arguments.is_empty() {
                Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&call.arguments)?
            };
            events.push(StreamEvent::ToolUse {
                id: call.id.unwrap_or_default(),
                name: call.name.unwrap_or_default(),
                input,
            });
        }
        Ok(())
    }
}

/// Decode a raw `/chat/completions` byte stream into `StreamEvent`s.
/// Same transport guarantees as the Anthropic decoder: a stream
/// without bytes is `EmptyResponse`, the first error ends it.
pub(crate) fn decode_chat_stream<S>(
    bytes: S,
) -> impl Stream<Item = Result<StreamEvent, Error>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, Error>> + Send + 'static,
{
    async_stream::stream! {
        let mut decoder = ChunkDecoder::default();
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
                    Ok(Some(line)) => match decoder.feed_line(&line) {
                        Ok((events, done)) => {
                            for event in events {
                                yield Ok(event);
                            }
                            if done {
                                return;
                            }
                        }
                        Err(e) => { yield Err(e); return; }
                    },
                    Ok(None) => break,
                    Err(e) => { yield Err(e); return; }
                }
            }
        }

        if !received_any_bytes {
            yield Err(Error::EmptyResponse);
            return;
        }
        if let Err(e) = acc.finish() {
            yield Err(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn decode_fixture(body: &'static str) -> Vec<Result<StreamEvent, Error>> {
        // Split into small chunks so lines straddle chunk boundaries.
        let chunks: Vec<Result<bytes::Bytes, Error>> = body
            .as_bytes()
            .chunks(17)
            .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
            .collect();
        decode_chat_stream(tokio_stream::iter(chunks))
            .collect()
            .await
    }

    fn ok_events(results: Vec<Result<StreamEvent, Error>>) -> Vec<StreamEvent> {
        results
            .into_iter()
            .map(|r| r.expect("no error expected"))
            .collect()
    }

    /// Text deltas, usage with zero cache counters, then stop.
    #[tokio::test]
    async fn text_stream_decodes_deltas_usage_stop() {
        let events = ok_events(
            decode_fixture(include_str!("../../../tests/fixtures/openai_text.sse")).await,
        );
        assert_eq!(events.len(), 5, "got {events:?}");
        assert!(matches!(&events[0], StreamEvent::ContentDelta(t) if t == "Hel"));
        assert!(matches!(&events[1], StreamEvent::ContentDelta(t) if t == "lo"));
        assert!(matches!(
            events[2],
            StreamEvent::Usage {
                input_tokens: 9,
                output_tokens: 2,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            }
        ));
        assert!(matches!(events[3], StreamEvent::Stop(StopReason::EndTurn)));
        assert!(matches!(events[4], StreamEvent::MessageStop));
    }

    /// Split argument fragments are joined into one `ToolUse`.
    #[tokio::test]
    async fn split_tool_call_is_buffered_into_one_tool_use() {
        let events = ok_events(
            decode_fixture(include_str!(
                "../../../tests/fixtures/openai_tool_split.sse"
            ))
            .await,
        );
        assert_eq!(events.len(), 3, "got {events:?}");
        assert!(
            matches!(&events[0], StreamEvent::ToolUse { id, name, input }
                if id == "call_1" && name == "read" && *input == json!({"path": "a.rs"})),
            "got {events:?}"
        );
        assert!(matches!(events[1], StreamEvent::Stop(StopReason::ToolUse)));
        assert!(matches!(events[2], StreamEvent::MessageStop));
    }

    /// Invalid argument JSON is `Err(Json)`; no `ToolUse` escapes.
    #[tokio::test]
    async fn invalid_tool_arguments_are_json_error() {
        let results =
            decode_fixture(include_str!("../../../tests/fixtures/openai_tool_bad.sse")).await;
        assert!(
            results
                .iter()
                .all(|r| !matches!(r, Ok(StreamEvent::ToolUse { .. }))),
            "no ToolUse may be emitted: {results:?}"
        );
        assert!(
            matches!(results.last(), Some(Err(Error::Json(_)))),
            "expected trailing Err(Json), got {results:?}"
        );
    }

    /// Empty arguments become `{}`; calls flush in index order; a stream
    /// without `[DONE]` emits no `MessageStop`.
    #[tokio::test]
    async fn empty_arguments_index_order_and_no_done() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"two\",\"arguments\":\"\"}},",
            "{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"one\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n"
        );
        let events = ok_events(decode_fixture(body).await);
        assert_eq!(events.len(), 2, "no MessageStop without [DONE]: {events:?}");
        assert!(
            matches!(&events[0], StreamEvent::ToolUse { id, name, input }
            if id == "a" && name == "one" && *input == json!({}))
        );
        assert!(
            matches!(&events[1], StreamEvent::ToolUse { id, name, input }
            if id == "b" && name == "two" && *input == json!({}))
        );
    }

    /// An in-stream `error` object is `Api` with kind `Other`, no status.
    #[tokio::test]
    async fn in_stream_error_is_api_other() {
        let body = "data: {\"error\":{\"message\":\"model overloaded\"}}\n\ndata: [DONE]\n\n";
        let results = decode_fixture(body).await;
        assert_eq!(results.len(), 1, "error ends the stream: {results:?}");
        match &results[0] {
            Err(Error::Api(e)) => {
                assert_eq!(e.status, None);
                assert_eq!(e.kind, crate::provider::error::ErrorKind::Other);
                assert_eq!(e.message, "model overloaded");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    /// Zero bytes keep the `"empty response"` guard.
    #[tokio::test]
    async fn empty_body_is_empty_response() {
        let results = decode_fixture("").await;
        assert!(
            matches!(&results[..], [Err(Error::EmptyResponse)]),
            "got {results:?}"
        );
    }

    /// Index-less parallel calls are kept apart by `id`, in
    /// creation order; the third chunk continues `call_a`.
    #[tokio::test]
    async fn indexless_parallel_calls_split_by_id() {
        let events = ok_events(
            decode_fixture(include_str!(
                "../../../tests/fixtures/openai_tool_noindex.sse"
            ))
            .await,
        );
        assert_eq!(events.len(), 4, "got {events:?}");
        assert!(
            matches!(&events[0], StreamEvent::ToolUse { id, name, input }
                if id == "call_a" && name == "fa" && *input == json!({"x": 1})),
            "got {events:?}"
        );
        assert!(
            matches!(&events[1], StreamEvent::ToolUse { id, name, input }
                if id == "call_b" && name == "fb" && *input == json!({"y": 2})),
            "got {events:?}"
        );
        assert!(matches!(events[2], StreamEvent::Stop(StopReason::ToolUse)));
        assert!(matches!(events[3], StreamEvent::MessageStop));
    }

    /// A fragment with neither `index` nor `id` continues the
    /// last slot.
    #[tokio::test]
    async fn fragment_without_index_or_id_continues_last() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"id\":\"call_c\",\"function\":{\"name\":\"fc\",\"arguments\":\"{\\\"z\\\":\"}}",
            "]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"function\":{\"arguments\":\"3}\"}}",
            "]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = ok_events(decode_fixture(body).await);
        let tool_uses: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::ToolUse { .. }))
            .collect();
        assert_eq!(tool_uses.len(), 1, "got {events:?}");
        assert!(
            matches!(tool_uses[0], StreamEvent::ToolUse { id, name, input }
                if id == "call_c" && name == "fc" && *input == json!({"z": 3})),
            "got {events:?}"
        );
    }

    /// `cached_tokens` are cache reads; `input_tokens` is the
    /// uncached rest.
    #[tokio::test]
    async fn cached_tokens_are_cache_reads() {
        let body = concat!(
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,",
            "\"prompt_tokens_details\":{\"cached_tokens\":80}}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = ok_events(decode_fixture(body).await);
        assert!(
            matches!(
                events[0],
                StreamEvent::Usage {
                    input_tokens: 20,
                    output_tokens: 5,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 80,
                }
            ),
            "got {events:?}"
        );
    }

    /// Every OpenAI finish reason; `Stop` second-to-last,
    /// `MessageStop` last; the first non-null reason wins.
    #[tokio::test]
    async fn openai_finish_reason_table() {
        for (wire, want) in [
            ("stop", StopReason::EndTurn),
            ("tool_calls", StopReason::ToolUse),
            ("length", StopReason::MaxTokens),
            ("content_filter", StopReason::Refusal),
            ("brand_new", StopReason::Unknown),
        ] {
            let body = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"Hi\"}},\"finish_reason\":null}}]}}\n\n\
                 data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"{wire}\"}}]}}\n\n\
                 data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
                 data: [DONE]\n\n"
            );
            let events = ok_events(decode_fixture(Box::leak(body.into_boxed_str())).await);
            let n = events.len();
            assert!(
                matches!(events[n - 2], StreamEvent::Stop(r) if r == want),
                "{wire}: {events:?}"
            );
            assert!(matches!(events[n - 1], StreamEvent::MessageStop));
        }
    }

    /// Tool arguments beyond 1 MiB end the stream with the cap error.
    #[tokio::test]
    async fn tool_input_cap() {
        let mut body = String::from(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"Write\",\"arguments\":\"\"}}]}}]}\n\n",
        );
        let frag = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":\"{}\"}}}}]}}}}]}}\n",
            "a".repeat(1024)
        );
        for _ in 0..2048 {
            body.push_str(&frag);
        }
        let chunks: Vec<Result<bytes::Bytes, Error>> = vec![Ok(bytes::Bytes::from(body))];
        let items: Vec<_> = decode_chat_stream(tokio_stream::iter(chunks))
            .collect()
            .await;
        assert!(items
            .iter()
            .all(|r| !matches!(r, Ok(StreamEvent::ToolUse { .. }))));
        match items.last().expect("items") {
            Err(Error::TooLarge { what, max }) => {
                assert_eq!((*what, *max), (Oversize::ToolInput, MAX_TOOL_INPUT_BYTES))
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }
}
