//! Level-0 path: a provider from this crate with no feature enabled.

use llm_provider::provider::{create_provider, AuthMethod, ProviderConfig};

#[test]
fn level0_create_provider_anthropic() {
    let provider = create_provider(
        &ProviderConfig::default(),
        Some(AuthMethod::ApiKey("test".into())),
    )
    .expect("level-0 factory builds an Anthropic provider");
    assert_eq!(provider.info().name, "anthropic");
}
