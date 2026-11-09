//! Tests for the `/chat/completions` stream decoder: text, usage, tool-call
//! assembly, finish reasons, error chunks and the guards.

use serde_json::json;

use super::*;
use crate::provider::completion::ToolUseCall;
use crate::provider::error::{ErrorKind, Oversize};
use crate::provider::openai::tool_calls::MAX_TOOL_INPUT_BYTES;

/// Decodes `body` split into 17-byte chunks, so lines straddle chunks.
async fn decode_body(body: &str) -> Vec<Result<StreamEvent, Error>> {
    let chunks: Vec<Result<bytes::Bytes, Error>> = body
        .as_bytes()
        .chunks(17)
        .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
        .collect();

    decode(Box::pin(tokio_stream::iter(chunks))).collect().await
}

/// The events of `body`, which must decode without error.
async fn ok_events(body: &str) -> Vec<StreamEvent> {
    decode_body(body)
        .await
        .into_iter()
        .map(|r| r.expect("no error expected"))
        .collect()
}

/// A `ToolUse` event.
fn tool_use(id: &str, name: &str, input: serde_json::Value) -> StreamEvent {
    StreamEvent::ToolUse(ToolUseCall {
        id: id.to_string(),
        name: name.to_string(),
        input,
    })
}

/// A usage event with input and output counts only.
fn usage(input_tokens: u32, output_tokens: u32) -> StreamEvent {
    StreamEvent::Usage(Usage {
        input_tokens,
        output_tokens,
        ..Usage::default()
    })
}

/// A text chunk, a chunk finishing with `reason`, then `[DONE]`.
fn finished_with(reason: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"Hi\"}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"{reason}\"}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
         data: [DONE]\n\n"
    )
}

/// Text deltas, usage with zero cache counters, then stop.
#[tokio::test]
async fn text_stream_decodes_deltas_usage_stop() {
    let events = ok_events(include_str!("../../../../tests/fixtures/openai_text.sse")).await;

    let expected = vec![
        StreamEvent::ContentDelta("Hel".to_string()),
        StreamEvent::ContentDelta("lo".to_string()),
        usage(9, 2),
        StreamEvent::Stop(StopReason::EndTurn),
        StreamEvent::MessageStop,
    ];
    assert_eq!(events, expected);
}

/// Split argument fragments are joined into one `ToolUse`.
#[tokio::test]
async fn split_tool_call_is_buffered_into_one_tool_use() {
    let events = ok_events(include_str!(
        "../../../../tests/fixtures/openai_tool_split.sse"
    ))
    .await;

    let expected = vec![
        tool_use("call_1", "read", json!({"path": "a.rs"})),
        StreamEvent::Stop(StopReason::ToolUse),
        StreamEvent::MessageStop,
    ];
    assert_eq!(events, expected);
}

/// Invalid argument JSON is `Err(Json)`; no `ToolUse` escapes.
#[tokio::test]
async fn invalid_tool_arguments_are_json_error() {
    let results = decode_body(include_str!(
        "../../../../tests/fixtures/openai_tool_bad.sse"
    ))
    .await;

    assert!(
        results
            .iter()
            .all(|r| !matches!(r, Ok(StreamEvent::ToolUse(_)))),
        "no ToolUse may be emitted: {results:?}"
    );
    assert!(
        matches!(results.last(), Some(Err(Error::Json(_)))),
        "got {results:?}"
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

    let events = ok_events(body).await;

    let expected = vec![
        tool_use("a", "one", json!({})),
        tool_use("b", "two", json!({})),
    ];
    assert_eq!(events, expected);
}

/// An in-stream error without a type is `Api` with kind `Other`, no status.
#[tokio::test]
async fn in_stream_error_is_api_other() {
    let body = "data: {\"error\":{\"message\":\"model overloaded\"}}\n\ndata: [DONE]\n\n";

    let results = decode_body(body).await;

    assert_eq!(results.len(), 1, "error ends the stream: {results:?}");
    match &results[0] {
        Err(Error::Api(e)) => {
            assert_eq!(e.status(), None);
            assert_eq!(e.kind(), ErrorKind::Other);
            assert_eq!(e.message(), "model overloaded");
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

/// A-8: an error chunk with `type: invalid_request_error` is `Api` with kind
/// `InvalidRequest` and message `bad`, read through the shared envelope.
#[tokio::test]
async fn error_chunk_type_decides_kind() {
    let body = "data: {\"error\":{\"message\":\"bad\",\"type\":\"invalid_request_error\"}}\n\n";

    let results = decode_body(body).await;

    match results.as_slice() {
        [Err(Error::Api(e))] => {
            assert_eq!((e.kind(), e.message()), (ErrorKind::InvalidRequest, "bad"));
        }
        other => panic!("expected one Api error, got {other:?}"),
    }
}

/// Zero bytes keep the `"empty response"` guard.
#[tokio::test]
async fn empty_body_is_empty_response() {
    let results = decode_body("").await;

    assert!(
        matches!(&results[..], [Err(Error::EmptyResponse)]),
        "got {results:?}"
    );
}

/// Index-less parallel calls are kept apart by `id`, in creation order; the
/// third chunk continues `call_a`.
#[tokio::test]
async fn indexless_parallel_calls_split_by_id() {
    let events = ok_events(include_str!(
        "../../../../tests/fixtures/openai_tool_noindex.sse"
    ))
    .await;

    let expected = vec![
        tool_use("call_a", "fa", json!({"x": 1})),
        tool_use("call_b", "fb", json!({"y": 2})),
        StreamEvent::Stop(StopReason::ToolUse),
        StreamEvent::MessageStop,
    ];
    assert_eq!(events, expected);
}

/// A fragment with neither `index` nor `id` continues the last call.
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

    let events = ok_events(body).await;

    let expected = vec![
        tool_use("call_c", "fc", json!({"z": 3})),
        StreamEvent::Stop(StopReason::ToolUse),
        StreamEvent::MessageStop,
    ];
    assert_eq!(events, expected);
}

/// `cached_tokens` are cache reads; `input_tokens` is the uncached rest.
#[tokio::test]
async fn cached_tokens_are_cache_reads() {
    let body = concat!(
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,",
        "\"prompt_tokens_details\":{\"cached_tokens\":80}}}\n\n",
        "data: [DONE]\n\n"
    );

    let events = ok_events(body).await;

    let cached = StreamEvent::Usage(Usage {
        input_tokens: 20,
        output_tokens: 5,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 80,
    });
    assert_eq!(events, vec![cached, StreamEvent::MessageStop]);
}

/// Every finish reason; `Stop` second to last, `MessageStop` last; the
/// first non-null reason wins.
#[tokio::test]
async fn openai_finish_reason_table() {
    for (wire, want) in [
        ("stop", StopReason::EndTurn),
        ("tool_calls", StopReason::ToolUse),
        ("length", StopReason::MaxTokens),
        ("content_filter", StopReason::Refusal),
        ("brand_new", StopReason::Unknown),
    ] {
        let events = ok_events(&finished_with(wire)).await;

        let tail = &events[events.len() - 2..];
        assert_eq!(
            tail,
            [StreamEvent::Stop(want), StreamEvent::MessageStop],
            "{wire}"
        );
    }
}

/// A-2: `length`, `content_filter` and `other` read as `MaxTokens`,
/// `Refusal` and `Unknown`.
#[tokio::test]
async fn finish_reasons_length_content_filter_other() {
    let mut stops = Vec::new();
    for wire in ["length", "content_filter", "other"] {
        let events = ok_events(&finished_with(wire)).await;
        stops.push(events[events.len() - 2].clone());
    }

    let expected = [
        StopReason::MaxTokens,
        StopReason::Refusal,
        StopReason::Unknown,
    ];
    assert_eq!(stops, expected.map(StreamEvent::Stop));
}

/// A-4: tool arguments beyond 1 MiB end the stream with the cap error.
#[tokio::test]
async fn tool_input_cap() {
    let mut body = String::from(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"Write\",\"arguments\":\"\"}}]}}]}\n\n",
    );
    let fragment = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":\"{}\"}}}}]}}}}]}}\n",
        "a".repeat(1024)
    );
    for _ in 0..2048 {
        body.push_str(&fragment);
    }
    let chunks: Vec<Result<bytes::Bytes, Error>> = vec![Ok(bytes::Bytes::from(body))];

    let items: Vec<_> = decode(Box::pin(tokio_stream::iter(chunks))).collect().await;

    assert!(items
        .iter()
        .all(|r| !matches!(r, Ok(StreamEvent::ToolUse(_)))));
    match items.last().expect("items") {
        Err(Error::TooLarge { what, max }) => {
            assert_eq!((*what, *max), (Oversize::ToolInput, MAX_TOOL_INPUT_BYTES));
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

/// Chunks as Bedrock's OpenAI-compatible route sends them for a reasoning
/// model: `reasoning_content` first, an empty first `content` with
/// `refusal: null`, `obfuscation` and `service_tier` on every chunk. The
/// extra fields decode without error and only `content` becomes text.
#[tokio::test]
async fn reasoning_and_extra_fields_are_ignored() {
    let body = "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"moonshotai.kimi-k3\",\"service_tier\":\"default\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\",\"refusal\":null},\"finish_reason\":null}],\"obfuscation\":\"a1b2\"}\n\n\
                data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"moonshotai.kimi-k3\",\"service_tier\":\"default\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"The user wants OK.\"},\"finish_reason\":null}],\"obfuscation\":\"c3d4e5\"}\n\n\
                data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"moonshotai.kimi-k3\",\"service_tier\":\"default\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}],\"obfuscation\":\"f6\"}\n\n\
                data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"moonshotai.kimi-k3\",\"service_tier\":\"default\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"obfuscation\":\"g7h8\"}\n\n\
                data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"moonshotai.kimi-k3\",\"service_tier\":\"default\",\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":9,\"total_tokens\":21},\"obfuscation\":\"i9\"}\n\n\
                data: [DONE]\n\n";

    let events = ok_events(body).await;

    let text: Vec<&StreamEvent> = events
        .iter()
        .filter(|e| matches!(e, StreamEvent::ContentDelta(_)))
        .collect();
    assert_eq!(text, [&StreamEvent::ContentDelta("OK".to_string())]);
    assert!(events.contains(&usage(12, 9)), "{events:?}");
    assert!(
        events.contains(&StreamEvent::Stop(StopReason::EndTurn)),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&StreamEvent::MessageStop));
}
