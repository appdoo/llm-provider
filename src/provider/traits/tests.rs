//! Tests for the provided `complete`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::provider::event::StreamEvent;
use crate::provider::message::Message;
use crate::provider::request::ModelSelection;

/// A provider that counts `stream` calls and answers with one stop.
#[derive(Default)]
struct CountingProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmProvider for CountingProvider {
    fn stream(&self, _request: CompletionRequest) -> EventStream {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(tokio_stream::iter(vec![Ok(StreamEvent::MessageStop)]))
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "counting".to_string(),
            model: "m".to_string(),
        }
    }
}

/// `complete` streams exactly once.
#[tokio::test]
async fn complete_calls_stream_once() {
    let provider = CountingProvider::default();
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message::user("hello")]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
    };

    provider.complete(request).await.expect("complete");

    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
