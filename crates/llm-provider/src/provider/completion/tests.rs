//! Tests for `fold`: text, usage, stop reason, errors.

use super::*;
use crate::provider::api_error::ApiError;
use crate::provider::error::ErrorKind;

/// `fold_cancellable` with a token nobody cancels.
async fn fold(events: EventStream, on_delta: impl FnMut(&str) + Send) -> Result<Completion, Error> {
    fold_cancellable(events, on_delta, &CancellationToken::new()).await
}

/// An event stream of `events`.
fn events(events: Vec<Result<StreamEvent, Error>>) -> EventStream {
    Box::pin(tokio_stream::iter(events))
}

/// A usage event with input and output counts only.
fn usage(input_tokens: u32, output_tokens: u32) -> StreamEvent {
    StreamEvent::Usage(Usage {
        input_tokens,
        output_tokens,
        ..Usage::default()
    })
}

/// A tool call named `n` without input.
fn tool_use() -> StreamEvent {
    StreamEvent::ToolUse(ToolUseCall {
        id: "t".to_string(),
        name: "n".to_string(),
        input: serde_json::json!({}),
    })
}

/// The stop reason `script` folds to.
async fn stop_of(script: Vec<StreamEvent>) -> StopReason {
    let script = script.into_iter().map(Ok).collect();
    fold(events(script), |_| {}).await.expect("fold").stop
}

/// Text deltas concatenate and reach the callback, usage sums, stop is
/// `EndTurn`.
#[tokio::test]
async fn fold_collects_text_and_usage() {
    let script = vec![
        Ok(StreamEvent::ContentDelta("Hel".to_string())),
        Ok(StreamEvent::ContentDelta("lo".to_string())),
        Ok(usage(12, 2)),
        Ok(StreamEvent::MessageStop),
    ];
    let mut deltas = Vec::new();

    let completion = fold(events(script), |delta| deltas.push(delta.to_string()))
        .await
        .expect("fold");

    let expected = Completion {
        text: "Hello".to_string(),
        tool_uses: vec![],
        usage: Usage {
            input_tokens: 12,
            output_tokens: 2,
            ..Usage::default()
        },
        stop: StopReason::EndTurn,
    };
    assert_eq!(completion, expected);
    assert_eq!(deltas, ["Hel", "lo"]);
}

/// A-9: `Usage(3/0)` and `Usage(0/2)` add up to 3/2; without `Stop` a tool
/// use means `ToolUse`, else `EndTurn`; a `Stop` event wins.
#[tokio::test]
async fn fold_sums_usage_and_infers_stop() {
    let script = vec![
        Ok(usage(3, 0)),
        Ok(usage(0, 2)),
        Ok(StreamEvent::MessageStop),
    ];
    let completion = fold(events(script), |_| {}).await.expect("fold");
    assert_eq!(completion.usage, usage_counts(3, 2));
    assert_eq!(completion.stop, StopReason::EndTurn);

    assert_eq!(
        stop_of(vec![tool_use(), StreamEvent::MessageStop]).await,
        StopReason::ToolUse
    );
    assert_eq!(
        stop_of(vec![
            StreamEvent::Stop(StopReason::MaxTokens),
            StreamEvent::MessageStop
        ])
        .await,
        StopReason::MaxTokens
    );
}

/// The `Usage` with input and output counts only.
fn usage_counts(input_tokens: u32, output_tokens: u32) -> Usage {
    Usage {
        input_tokens,
        output_tokens,
        ..Usage::default()
    }
}

/// Usage adds saturating.
#[test]
fn usage_add_assign_saturates() {
    let mut total = usage_counts(u32::MAX, 1);

    total += usage_counts(1, 1);

    assert_eq!(total, usage_counts(u32::MAX, 2));
}

/// The first stream error is returned unchanged.
#[tokio::test]
async fn fold_propagates_stream_error() {
    let overloaded = ApiError::new(ErrorKind::Overloaded, "overloaded");
    let script = vec![
        Ok(StreamEvent::ContentDelta("Hi".to_string())),
        Err(Error::Api(overloaded.clone())),
    ];

    match fold(events(script), |_| {}).await {
        Err(Error::Api(error)) => assert_eq!(error, overloaded),
        other => panic!("expected Api(overloaded), got {other:?}"),
    }
}

/// A-9: a stream that ends before `MessageStop` is `UnexpectedEnd`.
#[tokio::test]
async fn fold_without_message_stop_is_unexpected_end() {
    let script = vec![Ok(StreamEvent::ContentDelta("Hi".to_string()))];

    let result = fold(events(script), |_| {}).await;

    assert!(
        matches!(result, Err(Error::UnexpectedEnd)),
        "got {result:?}"
    );
}

/// C3: a stream that yields "a" and then hangs, with a token cancelled
/// after "a", is `Interrupted` with the partial text "a".
#[tokio::test]
async fn fold_cancellable_returns_the_partial_text() {
    let stream: EventStream = Box::pin(
        tokio_stream::iter(vec![Ok(StreamEvent::ContentDelta("a".to_string()))])
            .chain(tokio_stream::pending()),
    );
    let token = CancellationToken::new();
    let canceller = token.clone();

    let result = fold_cancellable(stream, move |_| canceller.cancel(), &token).await;

    match result {
        Err(Error::Interrupted { partial_text }) => assert_eq!(partial_text, "a"),
        other => panic!("expected Interrupted(\"a\"), got {other:?}"),
    }
}

// --- Cost ---

/// One million tokens in every counter, 400 000 of the cache write at 1h.
fn million_each() -> Usage {
    Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_creation_input_tokens: 1_000_000,
        cache_creation_1h_input_tokens: 400_000,
        cache_read_input_tokens: 1_000_000,
    }
}

/// P1: input 3, output 15, the cache write split 0.6 M at 3.75 and 0.4 M at
/// 6, the read at 0.3: 22.95 dollars.
#[test]
fn cost_prices_every_counter() {
    let cost = million_each().cost(&crate::Prices::per_million(3.0, 15.0));

    assert!((cost - 22.95).abs() < 1e-9, "{cost}");
}

/// P2: `regional(1.10)` multiplies everything: 25.245 dollars.
#[test]
fn cost_applies_the_regional_factor() {
    let prices = crate::Prices::per_million(3.0, 15.0).regional(1.10);

    let cost = million_each().cost(&prices);

    assert!((cost - 25.245).abs() < 1e-9, "{cost}");
}

/// Each derived cache price can be overridden: write 5m 4, write 1h 7,
/// read 0.5 gives 3 + 15 + 0.6×4 + 0.4×7 + 0.5 = 23.7 dollars.
#[test]
fn cost_uses_overridden_cache_prices() {
    let prices = crate::Prices::per_million(3.0, 15.0)
        .cache_write_5m(4.0)
        .cache_write_1h(7.0)
        .cache_read(0.5);

    let cost = million_each().cost(&prices);

    assert!((cost - 23.7).abs() < 1e-9, "{cost}");
}
