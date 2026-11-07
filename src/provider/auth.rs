//! The credential a caller passes to a provider.

use std::fmt;

use secrecy::SecretString;

/// A credential for a provider API. A plain string converts to
/// [`ApiKey`](Self::ApiKey); an OAuth token has to be named as
/// [`Bearer`](Self::Bearer).
///
/// ```no_run
/// use llm_provider::AuthMethod;
///
/// let key = AuthMethod::from("sk-ant-api03-…");
/// let token = AuthMethod::Bearer("sk-ant-oat01-…".into());
/// # let _ = (key, token);
/// ```
#[derive(Clone)]
#[non_exhaustive]
pub enum AuthMethod {
    /// An API key, sent as `x-api-key` (Anthropic) or `Authorization: Bearer`
    /// (OpenAI-compatible).
    ApiKey(SecretString),
    /// An OAuth access token (`sk-ant-oat01-…`), sent as
    /// `Authorization: Bearer` with the OAuth beta header.
    Bearer(SecretString),
}

impl From<String> for AuthMethod {
    fn from(key: String) -> Self {
        todo!()
    }
}

impl From<&str> for AuthMethod {
    fn from(key: &str) -> Self {
        todo!()
    }
}

/// Hand-written so the secret never reaches logs: prints `ApiKey("***")` or
/// `Bearer("***")`.
impl fmt::Debug for AuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

#[cfg(test)]
mod tests;
