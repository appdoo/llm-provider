//! The crate ignores ambient credentials: with `ANTHROPIC_API_KEY` set in
//! the process and no credential passed, building an Anthropic model
//! refuses. Alone in its file so the variable lives in its own process.

use llm_provider::config::ConfigError;
use llm_provider::{Error, ErrorKind, Llm};

#[test]
fn anthropic_without_credential_ignores_environment() {
    std::env::set_var("ANTHROPIC_API_KEY", "present");

    let result = Llm::builder("m").build();

    match result {
        Err(Error::Config(error @ ConfigError::MissingCredential { .. })) => {
            assert_eq!(
                error.to_string(),
                "the anthropic endpoint needs a credential"
            );
            assert_eq!(Error::Config(error).kind(), ErrorKind::Auth);
        }
        Err(other) => panic!("expected MissingCredential, got {other:?}"),
        Ok(_) => panic!("expected MissingCredential, got Ok"),
    }
}
