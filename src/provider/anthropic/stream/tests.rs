//! Tests for the event-stream decoder: the line parser and the byte-stream
//! decode with its empty-body guard and stop reasons.

use tokio_stream::StreamExt as _;

use super::*;

/// Feeds `lines` one by one and collects every event they complete.
fn feed_all(parser: &mut EventParser, lines: &[&str]) -> Vec<StreamEvent> {
    lines
        .iter()
        .flat_map(|line| parser.feed_line(line).expect("line"))
        .collect()
}

/// Feeds one whole event: its `event:` line, its `data:` line, the blank line.
fn feed_event(
    parser: &mut EventParser,
    event: &str,
    data: &str,
) -> Result<Vec<StreamEvent>, Error> {
    parser.feed_line(&format!("event: {event}"))?;
    parser.feed_line(&format!("data: {data}"))?;
    parser.feed_line("")
}

/// A byte stream of the given chunks.
fn byte_stream(chunks: Vec<&'static [u8]>) -> ByteStream {
    let items: Vec<Result<Bytes, Error>> = chunks
        .into_iter()
        .map(|c| Ok(Bytes::from_static(c)))
        .collect();
    Box::pin(tokio_stream::iter(items))
}

#[test]
fn parse_content_block_delta_text() {
    let mut parser = EventParser::default();
    assert!(parser
        .feed_line("event: content_block_delta")
        .unwrap()
        .is_empty());
    assert!(parser
        .feed_line(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#)
        .unwrap()
        .is_empty());

    match parser.feed_line("").unwrap().as_slice() {
        [StreamEvent::ContentDelta(text)] => assert_eq!(text, "Hi"),
        other => panic!("expected ContentDelta, got {other:?}"),
    }
}

#[test]
fn parse_message_stop() {
    let mut parser = EventParser::default();
    parser.feed_line("event: message_stop").unwrap();
    parser
        .feed_line(r#"data: {"type":"message_stop"}"#)
        .unwrap();

    match parser.feed_line("").unwrap().as_slice() {
        [StreamEvent::MessageStop] => {}
        other => panic!("expected MessageStop, got {other:?}"),
    }
}

#[test]
fn parse_tool_use_with_input() {
    let mut parser = EventParser::default();

    // content_block_start: buffers tool id+name
    parser.feed_line("event: content_block_start").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"bash"}}"#)
        .unwrap();
    assert!(
        parser.feed_line("").unwrap().is_empty(),
        "ToolUse should not emit at start"
    );

    // input_json_delta: accumulate input fragments
    parser.feed_line("event: content_block_delta").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\""}}"#)
        .unwrap();
    assert!(parser.feed_line("").unwrap().is_empty());

    parser.feed_line("event: content_block_delta").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":": \"ls\"}"}}"#)
        .unwrap();
    assert!(parser.feed_line("").unwrap().is_empty());

    // content_block_stop: emit complete ToolUse with parsed input
    parser.feed_line("event: content_block_stop").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_stop","index":1}"#)
        .unwrap();
    match parser.feed_line("").unwrap().as_slice() {
        [StreamEvent::ToolUse { id, name, input }] => {
            assert_eq!(id, "toolu_123");
            assert_eq!(name, "bash");
            assert_eq!(input["command"], "ls");
        }
        other => panic!("expected ToolUse with input, got {other:?}"),
    }
}

#[test]
fn parse_multiple_events() {
    let mut parser = EventParser::default();
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

    let events = feed_all(&mut parser, &lines);

    assert_eq!(events.len(), 3);
    match &events[0] {
        StreamEvent::ContentDelta(t) => assert_eq!(t, "Hello"),
        other => panic!("expected ContentDelta, got {other:?}"),
    }
    match &events[1] {
        StreamEvent::ContentDelta(t) => assert_eq!(t, " world"),
        other => panic!("expected ContentDelta, got {other:?}"),
    }
    assert!(matches!(&events[2], StreamEvent::MessageStop));
}

#[test]
fn partial_line_does_not_yield() {
    let mut parser = EventParser::default();
    // Feed event and data lines but no blank line to terminate
    assert!(parser
        .feed_line("event: content_block_delta")
        .unwrap()
        .is_empty());
    assert!(parser
        .feed_line(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}"#)
        .unwrap()
        .is_empty());
    // No blank line fed — no event should be emitted
}

#[test]
fn parse_message_start_usage() {
    let mut parser = EventParser::default();
    parser.feed_line("event: message_start").unwrap();
    parser
        .feed_line(r#"data: {"type":"message_start","message":{"usage":{"input_tokens":42}}}"#)
        .unwrap();

    match parser.feed_line("").unwrap().as_slice() {
        [StreamEvent::Usage {
            input_tokens,
            output_tokens,
            ..
        }] => {
            assert_eq!(*input_tokens, 42);
            assert_eq!(*output_tokens, 0);
        }
        other => panic!("expected Usage, got {other:?}"),
    }
}

#[test]
fn parse_message_delta_usage() {
    let mut parser = EventParser::default();
    parser.feed_line("event: message_delta").unwrap();
    parser
        .feed_line(r#"data: {"type":"message_delta","usage":{"output_tokens":100}}"#)
        .unwrap();

    match parser.feed_line("").unwrap().as_slice() {
        [StreamEvent::Usage {
            input_tokens,
            output_tokens,
            ..
        }] => {
            assert_eq!(*input_tokens, 0);
            assert_eq!(*output_tokens, 100);
        }
        other => panic!("expected Usage, got {other:?}"),
    }
}

/// `message_start` carries both cache counters from `message.usage`;
/// `output_tokens` is reported as 0.
#[test]
fn message_start_reports_cache_counters() {
    let mut parser = EventParser::default();
    assert!(parser.feed_line("event: message_start").unwrap().is_empty());
    assert!(parser
        .feed_line(r#"data: {"type":"message_start","message":{"usage":{"input_tokens":5,"cache_creation_input_tokens":1200,"cache_read_input_tokens":0}}}"#)
        .unwrap()
        .is_empty());

    match parser.feed_line("").unwrap().as_slice() {
        [StreamEvent::Usage {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
        }] => {
            assert_eq!(*input_tokens, 5);
            assert_eq!(*output_tokens, 0);
            assert_eq!(*cache_creation_input_tokens, 1200);
            assert_eq!(*cache_read_input_tokens, 0);
        }
        other => panic!("expected Usage, got {other:?}"),
    }
}

/// An `error` event ends in [`Error::Api`] without a status. The payload
/// carries `error.type`, which the typed wire error requires.
#[test]
fn parse_error_event() {
    let mut parser = EventParser::default();
    parser.feed_line("event: error").unwrap();
    parser
        .feed_line(r#"data: {"type":"error","error":{"type":"rate_limit_error","message":"rate limited"}}"#)
        .unwrap();

    match parser.feed_line("") {
        Err(Error::Api(e)) => {
            assert_eq!(e.message, "rate limited");
            assert_eq!(e.status, None);
        }
        other => panic!("expected Err(Api), got {other:?}"),
    }
}

/// `message_delta` stores the stop reason and still emits Usage; the stop
/// is handed out once, right before `MessageStop`.
#[test]
fn message_delta_stores_stop_reason() {
    let mut parser = EventParser::default();
    parser.feed_line("event: message_delta").unwrap();
    parser
        .feed_line(r#"data: {"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":5}}"#)
        .unwrap();
    assert!(matches!(
        parser.feed_line("").unwrap().as_slice(),
        [StreamEvent::Usage {
            output_tokens: 5,
            ..
        }]
    ));
    let stop = r#"{"type":"message_stop"}"#;
    assert!(matches!(
        feed_event(&mut parser, "message_stop", stop)
            .unwrap()
            .as_slice(),
        [
            StreamEvent::Stop(StopReason::MaxTokens),
            StreamEvent::MessageStop
        ]
    ));
    assert!(matches!(
        feed_event(&mut parser, "message_stop", stop)
            .unwrap()
            .as_slice(),
        [StreamEvent::MessageStop]
    ));
}

/// The Anthropic stop table, through the parser.
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
        let mut parser = EventParser::default();
        let delta = format!(r#"{{"type":"message_delta","delta":{{"stop_reason":"{wire}"}}}}"#);
        feed_event(&mut parser, "message_delta", &delta).unwrap();
        let events = feed_event(&mut parser, "message_stop", r#"{"type":"message_stop"}"#).unwrap();
        assert!(
            matches!(events.as_slice(), [StreamEvent::Stop(r), StreamEvent::MessageStop] if *r == want),
            "{wire}: {events:?}"
        );
    }
}

#[test]
fn ping_event_is_ignored() {
    let mut parser = EventParser::default();
    parser.feed_line("event: ping").unwrap();
    parser.feed_line(r#"data: {"type":"ping"}"#).unwrap();

    assert!(parser.feed_line("").unwrap().is_empty());
}

#[test]
fn empty_lines_without_data_are_ignored() {
    let mut parser = EventParser::default();
    // Multiple blank lines should not produce events
    assert!(parser.feed_line("").unwrap().is_empty());
    assert!(parser.feed_line("").unwrap().is_empty());
}

/// An unterminated `input_json_delta` fragment surfaces at
/// `content_block_stop` as [`Error::ToolInput`] naming the tool and id,
/// instead of a `ToolUse` with an empty input object.
#[test]
fn toolu_use_input_parse_error_propagates() {
    let mut parser = EventParser::default();

    // content_block_start: buffer tool id + name.
    parser.feed_line("event: content_block_start").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_bad","name":"bash"}}"#)
        .unwrap();
    assert!(parser.feed_line("").unwrap().is_empty());

    // input_json_delta: an unterminated JSON fragment.
    parser.feed_line("event: content_block_delta").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\""}}"#)
        .unwrap();
    assert!(parser.feed_line("").unwrap().is_empty());

    // content_block_stop: must now surface the tool-input error.
    parser.feed_line("event: content_block_stop").unwrap();
    parser
        .feed_line(r#"data: {"type":"content_block_stop","index":0}"#)
        .unwrap();

    match parser.feed_line("") {
        Err(Error::ToolInput { name, id, .. }) => {
            assert_eq!(id, "toolu_bad");
            assert_eq!(name, "bash");
        }
        other => panic!("expected Error::ToolInput, got {other:?}"),
    }
}

/// Tool input beyond 1 MiB fails on the 1 025th delta, no `ToolUse` before.
#[test]
fn tool_input_cap() {
    let mut parser = EventParser::default();
    let start = r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"Write"}}"#;
    assert!(feed_event(&mut parser, "content_block_start", start)
        .unwrap()
        .is_empty());
    let delta = format!(
        r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"input_json_delta","partial_json":"{}"}}}}"#,
        "a".repeat(1024)
    );
    for n in 1..=1025 {
        let r = feed_event(&mut parser, "content_block_delta", &delta);
        if n <= 1024 {
            assert!(
                matches!(&r, Ok(events) if events.is_empty()),
                "delta {n}: {r:?}"
            );
        } else {
            assert!(
                matches!(
                    r,
                    Err(Error::TooLarge {
                        what: Oversize::ToolInput,
                        max: 1_048_576
                    })
                ),
                "{r:?}"
            );
        }
    }
}

/// Joined `data:` lines beyond 1 MiB fail on the 1 025th line.
#[test]
fn event_cap() {
    let mut parser = EventParser::default();
    parser.feed_line("event: content_block_delta").unwrap();
    let line = format!("data: {}", "x".repeat(1024));
    for n in 1..=1025 {
        let r = parser.feed_line(&line);
        if n <= 1024 {
            assert!(
                matches!(&r, Ok(events) if events.is_empty()),
                "line {n}: {r:?}"
            );
        } else {
            assert!(
                matches!(
                    r,
                    Err(Error::TooLarge {
                        what: Oversize::Event,
                        max: 1_048_576
                    })
                ),
                "{r:?}"
            );
        }
    }
}

// --- HTTP 200 with empty body ---

/// Completely empty stream (no chunks at all).
#[tokio::test]
async fn decode_sse_empty_stream_yields_empty_response() {
    let mut events = decode(byte_stream(vec![]));

    let first = events.next().await.expect("expected one error event");
    assert!(matches!(first, Err(Error::EmptyResponse)), "got {first:?}");
    assert!(
        events.next().await.is_none(),
        "guard must terminate the stream after emitting the error"
    );
}

/// Stream emits only empty `Bytes` chunks (degenerate upstream): no *data*
/// bytes received is the same as no chunks at all.
#[tokio::test]
async fn decode_sse_only_empty_chunks_yields_empty_response() {
    let mut events = decode(byte_stream(vec![b"", b""]));

    let first = events.next().await.expect("expected one error event");
    assert!(matches!(first, Err(Error::EmptyResponse)), "got {first:?}");
    assert!(events.next().await.is_none());
}

/// A non-empty stream does not trigger the empty-body guard and produces
/// the expected event end-to-end.
#[tokio::test]
async fn decode_sse_non_empty_stream_does_not_trigger_empty_guard() {
    let payload: &'static [u8] = b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    let mut events = decode(byte_stream(vec![payload]));

    let mut saw_message_stop = false;
    while let Some(ev) = events.next().await {
        match ev {
            Ok(StreamEvent::MessageStop) => saw_message_stop = true,
            Err(Error::EmptyResponse) => {
                panic!("empty-body guard fired on a non-empty stream");
            }
            Ok(_) => {}
            Err(e) => panic!("unexpected error on well-formed stream: {e}"),
        }
    }
    assert!(
        saw_message_stop,
        "well-formed SSE payload must produce a MessageStop event"
    );
}

// --- stop reason ---

fn sse_body(stop_reason: &str) -> String {
    format!(
        "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"usage\":{{\"input_tokens\":3}}}}}}\n\n\
         event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"1, 2\"}}}}\n\n\
         event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":5}}}}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

async fn decode_all(body: String) -> Vec<StreamEvent> {
    let bytes: ByteStream = Box::pin(tokio_stream::iter(vec![Ok::<Bytes, Error>(Bytes::from(
        body,
    ))]));
    decode(bytes)
        .map(|r| r.expect("no error expected"))
        .collect()
        .await
}

/// Every Anthropic stop value; `Stop` is second-to-last and `MessageStop`
/// last.
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
