//! Settings for an [`OpenAi`] client, collected before it is built.

use std::sync::Arc;

use crate::provider::auth::AuthMethod;
use crate::provider::endpoint::{Endpoint, EndpointParts};
use crate::provider::openai::api::OpenAiChatApi;
use crate::provider::openai::client::OpenAi;
use crate::provider::options::ProviderOptions;
use crate::provider::retry::RetryPolicy;

/// Settings for an [`OpenAi`] client. Unset values take the defaults: no
/// credential (no `Authorization` header, as a local server wants),
/// [`ProviderOptions::default`] (connect 30 s, idle 90 s) and
/// [`RetryPolicy::default`] (4 attempts).
///
/// ```no_run
/// use llm_provider::{OpenAi, RetryPolicy};
///
/// let openai = OpenAi::builder("https://api.openai.com/v1")
///     .credential("sk-…")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = openai;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct OpenAiBuilder {
    parts: EndpointParts,
}

impl OpenAiBuilder {
    /// A builder with the defaults for the server at `url`; a trailing `/`
    /// is ignored.
    pub(crate) fn new(url: &str) -> Self {
        Self {
            parts: EndpointParts::new(url.trim_end_matches('/').to_string()),
        }
    }

    /// Sends `credential` as `Authorization: Bearer <key>`. A plain string is
    /// an API key.
    pub fn credential(mut self, credential: impl Into<AuthMethod>) -> Self {
        self.parts.credential = Some(credential.into());
        self
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        self.parts.options = options;
        self
    }

    /// Sends through `client`, one the caller configured with a proxy, root
    /// certificates or a shared pool, instead of a client built from
    /// [`timeouts`](Self::timeouts). Its timeouts and redirect policy are the
    /// caller's; the client this crate builds follows no redirect, so the
    /// credential never reaches a second origin.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.parts.client = Some(client);
        self
    }

    /// Sets the retry policy; [`RetryPolicy::none`] turns retry off.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.parts.retry = policy;
        self
    }

    /// The client. Its HTTP connection pool is built on first use, so
    /// building never fails and never touches the network.
    #[must_use]
    pub fn build(self) -> OpenAi {
        let endpoint = Arc::new(Endpoint::new(self.parts));
        let api = OpenAiChatApi::new(Arc::clone(&endpoint));

        OpenAi::from_chat(endpoint.retrying(api))
    }
}
