//! Tests for the provided `complete`.

use std::sync::Arc;

use super::*;
use crate::provider::event::StreamEvent;
use crate::provider::message::Message;
use crate::provider::request::ModelSelection;
use crate::provider::test_support::ScriptedProvider;

/// `complete` streams exactly once.
#[tokio::test]
async fn complete_calls_stream_once() {
    let provider = ScriptedProvider::new([vec![StreamEvent::MessageStop]]);
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message::user("hello")]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
        response_schema: None,
        sampling: crate::provider::Sampling::default(),
    };

    provider.complete(request).await.expect("complete");

    assert_eq!(provider.requests().len(), 1);
}
