//! Tests for the chat model over a caller's own provider and the masked
//! credential of every layer.

use super::*;
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{Anthropic, Bedrock, ModelSelection, OpenAi, StreamEvent};

/// The Bedrock API key every layer is built from.
const BEDROCK_KEY: &str = "ABSK-secret";

/// The OpenAI key every OpenAI layer is built from.
const OPENAI_KEY: &str = "sk-x";

/// A reply that streams `text` and ends.
fn text(text: &str) -> Vec<StreamEvent> {
    vec![
        StreamEvent::ContentDelta(text.into()),
        StreamEvent::MessageStop,
    ]
}

/// `value`'s `Debug`, checked to name `type_name` and to leave out `secret`.
fn assert_masked(value: &impl fmt::Debug, type_name: &str, secret: &str) {
    let debug = format!("{value:?}");

    assert!(debug.contains(type_name), "{type_name} missing: {debug}");
    assert!(!debug.contains(secret), "{type_name} leaks: {debug}");
}

/// P-12: no layer from an endpoint to a model prints its credential.
#[test]
fn every_layer_masks_its_credential() {
    let bedrock = Bedrock::new(BEDROCK_KEY, "eu-central-1");
    let openai = OpenAi::builder("http://127.0.0.1:9")
        .credential(OPENAI_KEY)
        .build();

    assert_masked(&bedrock, "Bedrock", BEDROCK_KEY);
    assert_masked(&bedrock.anthropic(), "Anthropic", BEDROCK_KEY);
    assert_masked(&bedrock.openai(), "OpenAi", BEDROCK_KEY);
    assert_masked(&bedrock.anthropic().llm("m"), "Llm", BEDROCK_KEY);
    assert_masked(&bedrock.openai().llm("m"), "Llm", BEDROCK_KEY);
    assert_masked(&bedrock.openai().embedder("m"), "Embedder", BEDROCK_KEY);
    assert_masked(&openai, "OpenAi", OPENAI_KEY);
    assert_masked(&openai.embedder("m"), "Embedder", OPENAI_KEY);
    let own = Llm::from_provider(bedrock.anthropic().llm("m"));
    assert_masked(&own, "Llm", BEDROCK_KEY);
    assert_masked(
        &Anthropic::builder(BEDROCK_KEY),
        "EndpointBuilder",
        BEDROCK_KEY,
    );
    assert_masked(
        &OpenAi::builder("http://127.0.0.1:9").credential(OPENAI_KEY),
        "EndpointBuilder",
        OPENAI_KEY,
    );
    assert_masked(
        &Bedrock::builder(BEDROCK_KEY, "eu-central-1"),
        "EndpointBuilder",
        BEDROCK_KEY,
    );
}

/// P-13: a caller's provider answers `ask`, names the model, and sees the
/// request's model selection as sent.
#[tokio::test]
async fn own_provider_sees_requests_unchanged() {
    let provider = ScriptedProvider::new([text("one"), text("two")]);
    let llm = Llm::from_provider(provider.clone());

    let answer = llm.ask("q").await.expect("ask");
    llm.request("q").model("n").send().await.expect("send");

    assert_eq!(answer, "one");
    assert_eq!(llm.model(), "scripted-1");
    assert_eq!(llm.info(), provider.info());
    let models: Vec<ModelSelection> = provider.requests().into_iter().map(|r| r.model).collect();
    assert_eq!(
        models,
        [ModelSelection::Default, ModelSelection::Named("n".into())]
    );
}
