//! Test-only [`ScriptedProvider`]: a provider that plays one event list per
//! call and records every request it receives.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::provider::{
    CompletionRequest, Error, EventStream, LlmProvider, ProviderInfo, StreamEvent,
};

/// The events of one model call, in order; an `Err` ends the stream.
pub(crate) type Script = Vec<Result<StreamEvent, Error>>;

/// A [`LlmProvider`] that answers call `n` with the `n`-th script. Clones
/// share the scripts and the request log, so a test keeps a clone to read
/// the requests after the provider has moved into a runtime.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScriptedProvider {
    scripts: Arc<Mutex<VecDeque<Script>>>,
    requests: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl ScriptedProvider {
    /// A provider that plays `scripts`, one per call, each event `Ok`.
    pub(crate) fn new(scripts: impl IntoIterator<Item = Vec<StreamEvent>>) -> Self {
        Self::with_results(
            scripts
                .into_iter()
                .map(|events| events.into_iter().map(Ok).collect()),
        )
    }

    /// A provider that plays `scripts`, one per call, errors included.
    pub(crate) fn with_results(scripts: impl IntoIterator<Item = Script>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            requests: Arc::default(),
        }
    }

    /// Every request received so far, in call order.
    pub(crate) fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().expect("request log poisoned").clone()
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        self.requests
            .lock()
            .expect("request log poisoned")
            .push(request);

        let script = self.scripts.lock().expect("scripts poisoned").pop_front();
        let script = script.expect("ScriptedProvider: no script left for this call");
        Box::pin(tokio_stream::iter(script))
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "scripted".to_string(),
            model: "scripted-1".to_string(),
        }
    }
}

/// The typed answer of the `send_as` and `send_json` tests: what a text is
/// about.
#[derive(Debug, PartialEq, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub(crate) struct Summary {
    /// Two to four sentences.
    pub(crate) sentences: Vec<String>,
    /// Three to five key points.
    pub(crate) key_points: Vec<String>,
}

impl Summary {
    /// Its schema written by hand, as a level-0 caller writes it.
    pub(crate) fn schema() -> crate::provider::ResponseSchema {
        crate::provider::ResponseSchema::new(
            "Summary",
            serde_json::json!({"type": "object",
                "properties": {"sentences": {"type": "array"}, "key_points": {"type": "array"}},
                "required": ["sentences", "key_points"]}),
        )
    }

    /// The answer `{"sentences":["a"],"key_points":["b"]}`.
    pub(crate) fn ab() -> Self {
        Self {
            sentences: vec!["a".to_string()],
            key_points: vec!["b".to_string()],
        }
    }
}
