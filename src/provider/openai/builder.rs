//! Settings for an [`OpenAiCompatible`] client, collected before it is built.

use crate::provider::auth::AuthMethod;
use crate::provider::openai::api::OpenAiChatApi;
use crate::provider::openai::client::OpenAiCompatible;
use crate::provider::options::ProviderOptions;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::transport::Http;

/// Settings for an [`OpenAiCompatible`] client. Unset values take the
/// defaults: no credential (no `Authorization` header, as a local server
/// wants), [`ProviderOptions::default`] (connect 30 s, idle 90 s) and
/// [`RetryPolicy::default`] (4 attempts).
///
/// ```no_run
/// use llm_provider::{OpenAiCompatible, RetryPolicy};
///
/// let openai = OpenAiCompatible::builder("https://api.openai.com/v1", "gpt-4o-mini")
///     .credential("sk-…")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = openai;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct OpenAiBuilder {
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) credential: Option<AuthMethod>,
    options: ProviderOptions,
    retry: RetryPolicy,
    http_client: Option<reqwest::Client>,
}

impl OpenAiBuilder {
    /// A builder with the defaults for `model` at `base_url`; a trailing `/`
    /// is ignored.
    pub(crate) fn new(base_url: &str, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            credential: None,
            options: ProviderOptions::default(),
            retry: RetryPolicy::default(),
            http_client: None,
        }
    }

    /// Sends `credential` as `Authorization: Bearer <key>`. A plain string is
    /// an API key.
    pub fn credential(mut self, credential: impl Into<AuthMethod>) -> Self {
        self.credential = Some(credential.into());
        self
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        self.options = options;
        self
    }

    /// Sends through `client`, one the caller configured with a proxy, root
    /// certificates or a shared pool, instead of a client built from
    /// [`timeouts`](Self::timeouts). Its timeouts and redirect policy are the
    /// caller's; the client this crate builds follows no redirect, so the
    /// credential never reaches a second origin.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Sets the retry policy; [`RetryPolicy::none`] turns retry off.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// The client. Its HTTP connection pool is built on first use, so
    /// building never fails and never touches the network.
    #[must_use]
    pub fn build(self) -> OpenAiCompatible {
        let retry = self.retry;
        OpenAiCompatible::from_retrying(RetryingProvider::new(self.api(), retry))
    }

    /// One attempt against the server with these settings, without retry.
    pub(crate) fn api(mut self) -> OpenAiChatApi {
        let options = self.options;
        let http = self
            .http_client
            .take()
            .map_or_else(|| Http::new(options), Http::from);

        OpenAiChatApi::new(self, http)
    }
}
