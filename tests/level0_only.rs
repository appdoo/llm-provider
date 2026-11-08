//! Level-0 path: a provider from this crate with no feature enabled.

use llm_provider::{Anthropic, LlmProvider};

#[test]
fn level0_anthropic_front_type() {
    let provider: Box<dyn LlmProvider> = Box::new(Anthropic::new("test", "claude-sonnet-4-6"));

    let info = provider.info();

    assert_eq!(info.name, "anthropic");
    assert_eq!(info.model, "claude-sonnet-4-6");
}
