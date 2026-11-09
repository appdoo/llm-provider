//! The crate ignores ambient credentials: with `ANTHROPIC_API_KEY` set in
//! the process and no credential passed, the Anthropic factory refuses.
//! Alone in its file so the variable lives in its own process.
#![cfg(feature = "config")]

use llm_provider::config::{create_provider, ConfigError, ProviderConfig};
use llm_provider::Error;

#[test]
fn anthropic_without_credential_ignores_environment() {
    std::env::set_var("ANTHROPIC_API_KEY", "present");
    let config = ProviderConfig::default();

    match create_provider(&config, None) {
        Err(Error::Config(error @ ConfigError::MissingCredential { .. })) => {
            assert_eq!(error.to_string(), "anthropic provider needs a credential");
        }
        Err(other) => panic!("expected MissingCredential, got {other:?}"),
        Ok(_) => panic!("expected MissingCredential, got Ok"),
    }
}
