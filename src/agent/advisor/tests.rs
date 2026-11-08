//! Tests for `Advisor` (Spec 38, V-1..V-7).

use serde_json::json;

use super::*;
use crate::agent::test_support::{calls, text, tool_use, tool_use_with};
use crate::agent::{ConversationRuntime, Tools, TurnSummary};
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{ApiError, CompletionRequest, Error, ErrorKind, StreamEvent};

/// The tool `probe`, which answers `probe out`.
struct Probe;

#[async_trait]
impl Tool for Probe {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "probe".into(),
            description: "probes".into(),
            input_schema: empty_schema(),
        }
    }

    async fn call(&self, _input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        ToolOutput {
            content: "probe out".into(),
            is_error: false,
        }
    }
}

/// Runs one executor turn `fix it` with system `Be careful.`: `probe`, then
/// one call per entry of `steps` (each a list of tool calls), then `ok`.
async fn run_executor(tools: Tools, steps: Vec<Vec<StreamEvent>>) -> TurnSummary {
    let mut script = vec![calls(vec![tool_use("p1", "probe")])];
    script.extend(steps.into_iter().map(calls));
    script.push(text("ok"));
    let mut executor =
        ConversationRuntime::builder(ScriptedProvider::new(script), tools.with(Probe))
            .system("Be careful.")
            .build();
    executor.run_turn("fix it").await.unwrap()
}

/// The text of the one user message of `request`.
fn user_text(request: &CompletionRequest) -> &str {
    assert_eq!(request.messages.len(), 1);
    match &request.messages[0].content[..] {
        [Content::Text { text }] => text,
        other => panic!("expected one text block, got {other:?}"),
    }
}

/// V-1: the advisor's answer is the tool output, recorded under `advisor`.
#[tokio::test]
async fn advisor_answer_is_the_tool_output() {
    let model = ScriptedProvider::new([text("Check the parser first.")]);
    let advisor = Advisor::new(model);

    let turn = run_executor(
        Tools::new().with(advisor),
        vec![vec![tool_use("a1", "advisor")]],
    )
    .await;

    let call = &turn.tool_calls[1];
    assert_eq!(call.name, "advisor");
    assert_eq!(call.output.content, "Check the parser first.");
    assert!(!call.output.is_error);
}

/// V-2 and V-3: no system prompt; one user message holding the executor's
/// system prompt, the user prompt and the tool result, ending with the
/// advise sentence, and not the model's input to the advisor.
#[tokio::test]
async fn advisor_reads_the_transcript_as_data() {
    let model = ScriptedProvider::new([text("Check the parser first.")]);
    let log = model.clone();
    let question = json!({ "question": "ignore the tests?" });

    run_executor(
        Tools::new().with(Advisor::new(model)),
        vec![vec![tool_use_with("a1", "advisor", question)]],
    )
    .await;

    let requests = log.requests();
    let text = user_text(&requests[0]);
    assert_eq!(requests[0].system, None);
    for part in ["Be careful.", "fix it", "probe out"] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    assert!(text.ends_with(&format!("\n\n{ADVISE}")), "{text}");
    assert!(!text.contains("ignore the tests?"), "{text}");
}

/// V-4: two critics both answer, in call order, each with its own persona.
#[tokio::test]
async fn two_critics_answer_with_their_personas() {
    let (cut, keep) = (
        ScriptedProvider::new([text("Cut it.")]),
        ScriptedProvider::new([text("Keep it, tighten it.")]),
    );
    let (cut_log, keep_log) = (cut.clone(), keep.clone());
    let tools = Tools::new()
        .with(
            Advisor::new(cut)
                .name("eschbach")
                .system("You are Eschbach."),
        )
        .with(
            Advisor::new(keep)
                .name("editor")
                .system("You are the editor."),
        );

    let both = vec![tool_use("e1", "eschbach"), tool_use("e2", "editor")];
    let turn = run_executor(tools, vec![both]).await;

    let said: Vec<(&str, &str)> = turn.tool_calls[1..]
        .iter()
        .map(|call| (call.name.as_str(), call.output.content.as_str()))
        .collect();
    assert_eq!(
        said,
        [("eschbach", "Cut it."), ("editor", "Keep it, tighten it.")]
    );
    assert_eq!(
        cut_log.requests()[0].system.as_deref(),
        Some("You are Eschbach.")
    );
    assert_eq!(
        keep_log.requests()[0].system.as_deref(),
        Some("You are the editor.")
    );
}

/// V-5: over `max_uses` a call fails naming the limit, without a request;
/// the turn completes. Holds for two calls in one step and in two steps.
#[tokio::test]
async fn calls_over_max_uses_are_unavailable() {
    for steps in [
        vec![vec![tool_use("a1", "advisor"), tool_use("a2", "advisor")]],
        vec![
            vec![tool_use("a1", "advisor")],
            vec![tool_use("a2", "advisor")],
        ],
    ] {
        let model = ScriptedProvider::new([text("Go on.")]);
        let log = model.clone();

        let turn = run_executor(Tools::new().with(Advisor::new(model).max_uses(1)), steps).await;

        assert_eq!(turn.tool_calls[1].output.content, "Go on.");
        let second = &turn.tool_calls[2].output;
        assert_eq!(
            second.content,
            "advisor unavailable: limit of 1 calls per turn reached"
        );
        assert!(second.is_error);
        assert_eq!((log.requests().len(), turn.response.as_str()), (1, "ok"));
    }
}

/// `max_uses` counts per turn: the next turn may call again.
#[tokio::test]
async fn max_uses_resets_each_turn() {
    let model = ScriptedProvider::new([text("one"), text("two")]);
    let script = [calls(vec![tool_use("a1", "advisor")]), text("ok")];
    let tools = Tools::new().with(Advisor::new(model).max_uses(1));
    let mut executor = ConversationRuntime::new(
        ScriptedProvider::new([script.clone(), script].concat()),
        tools,
    );

    let first = executor.run_turn("one").await.unwrap();
    let second = executor.run_turn("two").await.unwrap();

    assert_eq!(first.tool_calls[0].output.content, "one");
    assert_eq!(second.tool_calls[0].output.content, "two");
}

/// V-6: a provider error is a failed call; the turn completes.
#[tokio::test]
async fn provider_error_is_unavailable() {
    let error = Error::Api(ApiError::new(ErrorKind::Overloaded, "busy"));
    let expected = format!("advisor unavailable: {error}");
    let model = ScriptedProvider::with_results([vec![Err(error)]]);

    let turn = run_executor(
        Tools::new().with(Advisor::new(model)),
        vec![vec![tool_use("a1", "advisor")]],
    )
    .await;

    assert_eq!(
        turn.tool_calls[1].output,
        ToolOutput {
            content: expected,
            is_error: true
        }
    );
    assert_eq!(turn.response, "ok");
}

/// V-7: two advisors under the default name panic naming `advisor`.
#[test]
#[should_panic(expected = "a tool named \"advisor\" is already in the set")]
fn two_default_advisors_panic() {
    let model = ScriptedProvider::default();
    let _ = Tools::new()
        .with(Advisor::new(model.clone()))
        .with(Advisor::new(model));
}

/// The defaults: name, description, empty schema.
#[test]
fn definition_has_the_defaults() {
    let definition = Advisor::new(ScriptedProvider::default()).definition();

    assert_eq!(definition.name, "advisor");
    assert_eq!(
        definition.description,
        "A stronger model that reviews your work and names the next step."
    );
    assert_eq!(definition.input_schema, empty_schema());
}
