//! The caller programs of the advisor and agent-as-tool examples against scripted providers.

use std::fmt::Write as _;

use crate::agent::test_support::{calls, scripted, text, tool_use};
use crate::agent::{Advisor, ConversationRuntime, Tool, Tools, Transcript};
use crate::provider::test_stub::StubServer;
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{Anthropic, Llm, OpenAi, RetryPolicy};

/// A recorded Anthropic stream answering "Hi".
const ANTHROPIC_SSE: &str = include_str!("../../tests/fixtures/anthropic_text.sse");

/// A recorded OpenAI stream answering "Hello".
const OPENAI_SSE: &str = include_str!("../../tests/fixtures/openai_text.sse");

/// A stand-in for a web tool block.
#[cfg(feature = "fs-tools")]
struct Web;

/// What to look up.
#[cfg(feature = "fs-tools")]
#[derive(serde::Deserialize, crate::JsonSchema)]
struct Query {
    /// The question.
    q: String,
}

#[cfg(feature = "fs-tools")]
#[crate::agent::tools]
impl Web {
    /// Looks something up on the web.
    #[tool]
    async fn search(&self, query: Query) -> Result<String, std::convert::Infallible> {
        Ok(format!("results for {}", query.q))
    }
}

/// The tool names of a turn's calls, in order.
fn names(turn: &crate::agent::TurnSummary) -> Vec<&str> {
    turn.tool_calls
        .iter()
        .map(|call| call.name.as_str())
        .collect()
}

/// Opus works, Fable advises.
#[cfg(feature = "fs-tools")]
#[tokio::test]
async fn coder_consults_its_advisor() {
    use crate::agent::CONSULT_ADVISOR;
    use crate::fs_tools::{Files, Sandbox};

    let dir = tempfile::tempdir().unwrap();
    let fable = ScriptedProvider::new([text("Run the test first.")]);
    let opus = ScriptedProvider::new([calls(vec![tool_use("a1", "advisor")]), text("Fixed.")]);
    let log = opus.clone();

    let advisor = Advisor::new(scripted(fable)).max_uses(3);
    let tools = Tools::new()
        .with_all(Files::new(Sandbox::new(dir.path()).unwrap()))
        .with(advisor);
    let mut coder = ConversationRuntime::builder(scripted(opus), tools)
        .system(format!("You fix code.\n\n{CONSULT_ADVISOR}"))
        .build();
    let turn = coder
        .run_turn("Make the failing test in parser.rs pass.")
        .await
        .unwrap();

    assert_eq!(names(&turn), ["advisor"]);
    assert_eq!(turn.tool_calls[0].output.content, "Run the test first.");
    assert_eq!(turn.response, "Fixed.");
    assert!(log.requests()[0]
        .system
        .as_deref()
        .unwrap()
        .ends_with(CONSULT_ADVISOR));
}

/// A writer with two critics; their disagreement stays visible.
#[tokio::test]
async fn writer_hears_two_critics() {
    let critic = |answer| ScriptedProvider::new([text(answer)]);
    let demoauthor2 = Advisor::new(scripted(critic("Cut it.")))
        .name("demoauthor2")
        .description("Critic: plot logic, plausibility, what to cut.")
        .system("You are demoauthor2.");
    let editor = Advisor::new(scripted(critic("Keep it, tighten it.")))
        .name("editor")
        .description("Critic: rhythm, clarity, repetition.")
        .system("You are the editor.");
    let opus = ScriptedProvider::new([
        calls(vec![
            tool_use("e1", "demoauthor2"),
            tool_use("e2", "editor"),
        ]),
        text("Tightened, not cut."),
    ]);

    let mut writer =
        ConversationRuntime::builder(scripted(opus), Tools::new().with(demoauthor2).with(editor))
            .system("You are demoauthor1. Name disagreements between critics and decide.")
            .build();
    let turn = writer.run_turn("Revise chapter 1, scene 3.").await.unwrap();

    let mut printed = String::new();
    for call in &turn.tool_calls {
        writeln!(printed, "--- {} ---\n{}", call.name, call.output.content).unwrap();
    }
    println!("{printed}");
    assert_eq!(names(&turn), ["demoauthor2", "editor"]);
    assert_eq!(
        printed,
        "--- demoauthor2 ---\nCut it.\n--- editor ---\nKeep it, tighten it.\n"
    );
}

/// A lead that delegates to two agents and asks an advisor.
#[cfg(feature = "fs-tools")]
#[tokio::test]
async fn lead_delegates_and_asks() {
    use crate::agent::test_support::tool_use_with;
    use crate::fs_tools::{Files, Sandbox};
    use serde_json::json;

    let dir = tempfile::tempdir().unwrap();
    let haiku = ScriptedProvider::new([
        calls(vec![tool_use_with(
            "s1",
            "search",
            json!({ "q": "backon" }),
        )]),
        text("backon retries with a builder."),
    ]);
    let sonnet = ScriptedProvider::new([text("Retry added.")]);
    let fable = ScriptedProvider::new([text("Verify the tests.")]);
    let opus = ScriptedProvider::new([
        calls(vec![
            tool_use_with(
                "l1",
                "researcher",
                json!({ "prompt": "How does backon retry?" }),
            ),
            tool_use_with("l2", "coder", json!({ "prompt": "Add retry." })),
            tool_use("l3", "advisor"),
        ]),
        text("Done and verified."),
    ]);

    let researcher = ConversationRuntime::builder(scripted(haiku), Tools::new().with_all(Web))
        .system("You answer one research question with sources.")
        .build()
        .into_tool(
            "researcher",
            "Looks something up on the web. Give it one question.",
        );
    let coder = ConversationRuntime::builder(
        scripted(sonnet),
        Tools::new().with_all(Files::new(Sandbox::new(dir.path()).unwrap())),
    )
    .build()
    .into_tool("coder", "Edits files in src/. Give it one task.");
    let tools = Tools::new()
        .with(researcher)
        .with(coder)
        .with(Advisor::new(scripted(fable)));
    let mut lead = ConversationRuntime::builder(scripted(opus), tools)
        .system("You lead; delegate, then verify.")
        .build();
    let turn = lead
        .run_turn("Add retry to the HTTP client.")
        .await
        .unwrap();

    assert_eq!(names(&turn), ["researcher", "coder", "advisor"]);
    let outputs: Vec<&str> = turn
        .tool_calls
        .iter()
        .map(|call| call.output.content.as_str())
        .collect();
    assert_eq!(
        outputs,
        [
            "backon retries with a builder.",
            "Retry added.",
            "Verify the tests."
        ]
    );
    assert_eq!(turn.response, "Done and verified.");
}

/// P-10: models from two endpoints, held in one `Vec<Llm>`, drive a runtime
/// and an advisor; each answers with its stub's text.
#[tokio::test]
async fn models_of_two_endpoints_share_one_type() {
    let anthropic_stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", ANTHROPIC_SSE.to_string()),
    )])
    .await;
    let openai_stub = StubServer::start(vec![(
        ("POST", "/chat/completions"),
        (200, "text/event-stream", OPENAI_SSE.to_string()),
    )])
    .await;
    let models: Vec<Llm> = vec![
        Anthropic::builder("sk-ant")
            .base_url(anthropic_stub.url())
            .retry(RetryPolicy::none())
            .build()
            .llm("claude"),
        OpenAi::builder(openai_stub.url())
            .retry(RetryPolicy::none())
            .build()
            .llm("gpt"),
    ];

    let mut runtime = ConversationRuntime::new(models[0].clone(), Tools::new());
    let turn = runtime.run_turn("hi").await.expect("turn");
    let advisor = Advisor::new(models[1].clone());
    let transcript = Transcript {
        system: None,
        tools: &[],
        messages: runtime.history(),
    };
    let advice = advisor.call(serde_json::json!({}), &transcript).await;

    assert_eq!(turn.response, "Hi");
    assert_eq!(advice.content, "Hello");
    assert!(!advice.is_error);
}
