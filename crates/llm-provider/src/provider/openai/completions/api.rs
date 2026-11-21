//! [`CompletionsApi`]: one attempt over the wire to an OpenAI-compatible
//! `/completions` endpoint.

use serde_json::Value;

use crate::provider::openai::api::OpenAiApi;
use crate::provider::openai::completions::stream::decode;
use crate::provider::protocol::failed;
use crate::provider::EventStream;
use crate::provider::Protocol;

/// The path of the raw Completions endpoint below the API root.
const COMPLETIONS_PATH: &str = "/completions";

/// The refusal of a Bedrock channel, which has no `/completions`.
const NO_RAW_ON_BEDROCK: &str = "Bedrock has no raw completion";

/// One attempt against the `/completions` endpoint of an
/// OpenAI-compatible server. [`OpenAi::raw`](crate::OpenAi::raw) wraps it in
/// retry; clones share the channel.
#[derive(Clone, Debug)]
pub(crate) struct CompletionsApi {
    api: OpenAiApi,
}

impl CompletionsApi {
    /// The `/completions` endpoint of the server `api` talks to.
    pub(crate) fn new(api: OpenAiApi) -> Self {
        Self { api }
    }
}

impl Protocol for CompletionsApi {
    fn stream_body(&self, body: Value) -> EventStream {
        if let Err(error) = self.api.route().refuse_on_bedrock(NO_RAW_ON_BEDROCK) {
            return failed(error);
        }

        self.api.send_body(COMPLETIONS_PATH, body, decode)
    }

    fn name(&self) -> &'static str {
        "openai"
    }
}
