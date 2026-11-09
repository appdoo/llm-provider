//! Tests for `fold`: text, usage, stop reason, errors.

use super::*;
use crate::provider::api_error::ApiError;
use crate::provider::error::ErrorKind;

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
