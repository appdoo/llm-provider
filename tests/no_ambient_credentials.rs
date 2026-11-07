//! The crate ignores ambient credentials: with `ANTHROPIC_API_KEY` set in
//! the process and no credential passed, the Anthropic factory refuses.
//! Alone in its file so the variable lives in its own process.

use llm_provider::provider::{create_provider, Error, ProviderConfig};

#[test]
fn anthropic_without_credential_ignores_environment() {
    std::env::set_var("ANTHROPIC_API_KEY", "present");
    let config = ProviderConfig {
        default: "anthropic".into(),
        ..Default::default()
    };
    match create_provider(&config, None) {
        Err(Error::MissingCredentials { message }) => {
            assert_eq!(message, "anthropic provider needs a credential")
        }
        Err(other) => panic!("expected MissingCredentials, got {other:?}"),
        Ok(_) => panic!("expected MissingCredentials, got Ok"),
    }
}
