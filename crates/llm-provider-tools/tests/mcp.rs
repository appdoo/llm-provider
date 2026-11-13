//! MCP servers as tools, against the test server over stdio and over
//! Streamable HTTP.

mod support;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use llm_provider::agent::{ConversationRuntime, ToolOutput, Toolbox, Tools};
use llm_provider::provider::{StreamEvent, ToolUseCall};
use llm_provider::Llm;
use llm_provider_tools::mcp::{Error, McpServer};
use serde_json::json;

use support::test_server::FAIL_MESSAGE;
use support::{call, serve_http, stdio, CallLog, ScriptedModel};

const ALL_TOOLS: [&str; 6] = ["add", "echo", "env", "fail", "picture", "sleep"];

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

#[tokio::test]
async fn connect_lists_every_tool() {
    let tools = stdio().connect().await.expect("connect");

    assert_eq!(sorted(tools.names()), ALL_TOOLS);
}

#[tokio::test]
async fn echo_answers_the_text() {
    let tools = stdio().connect().await.expect("connect");

    let output = call(tools, "echo", json!({"text": "hi"})).await;

    assert_eq!(output, ToolOutput::ok("hi"));
}

#[tokio::test]
async fn the_input_object_is_sent_as_the_arguments() {
    let tools = stdio().connect().await.expect("connect");

    let output = call(tools, "add", json!({"a": 2, "b": 3})).await;

    assert_eq!(output.content, "5");
    assert!(!output.is_error);
}

#[tokio::test]
async fn the_server_error_flag_reaches_the_model() {
    let tools = stdio().connect().await.expect("connect");

    let output = call(tools, "fail", json!({})).await;

    assert_eq!(output, ToolOutput::error(FAIL_MESSAGE));
}

#[tokio::test]
async fn a_call_past_the_timeout_answers_is_error() {
    let tools = stdio()
        .timeout(Duration::from_secs(1))
        .connect()
        .await
        .expect("connect");

    let started = Instant::now();
    let output = call(tools, "sleep", json!({"ms": 5000})).await;

    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(output.is_error);
    assert_eq!(output.content, "call to sleep timed out after 1s");
}

#[tokio::test]
async fn a_declined_call_never_reaches_the_server() {
    let log = CallLog::new();
    let tools = log
        .stdio()
        .approve(|_, _| false)
        .connect()
        .await
        .expect("connect");

    let output = call(tools, "echo", json!({"text": "hi"})).await;

    assert_eq!(output, ToolOutput::error("call to echo declined"));
    assert_eq!(log.calls("echo"), 0);
}

#[tokio::test]
async fn approve_sees_the_tool_name_and_input_and_lets_the_call_through() {
    let log = CallLog::new();
    let tools = log
        .stdio()
        .approve(|tool, input| tool == "echo" && input == &json!({"text": "hi"}))
        .connect()
        .await
        .expect("connect");

    let output = call(tools, "echo", json!({"text": "hi"})).await;

    assert_eq!(output.content, "hi");
    assert_eq!(log.calls("echo"), 1);
}

#[tokio::test]
async fn an_image_block_shows_its_media_type() {
    let tools = stdio().connect().await.expect("connect");

    let output = call(tools, "picture", json!({})).await;

    assert_eq!(output.content, "[image image/png]");
    assert!(!output.is_error);
}

#[tokio::test]
async fn two_servers_with_the_same_tool_collide_by_name() {
    let first = stdio().connect().await.expect("connect");
    let second = stdio().connect().await.expect("connect");

    let collision = catch_unwind(AssertUnwindSafe(|| {
        Tools::new().with_all(first).with_all(second)
    }));

    let payload = collision.expect_err("a duplicate name panics");
    let message = payload
        .downcast_ref::<String>()
        .expect("a formatted message");
    assert!(message.contains("\"add\""), "{message}");
}

#[tokio::test]
async fn a_prefix_keeps_two_servers_apart() {
    let first = stdio().connect().await.expect("connect");
    let second = stdio().prefix("b").connect().await.expect("connect");

    assert!(second.names().contains(&"b_add".to_string()));
    let tools = Tools::new().with_all(first).with_all(second);
    let names: Vec<String> = tools
        .definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect();

    assert_eq!(names.len(), 12);
    assert!(names.contains(&"add".to_string()));
    assert!(names.contains(&"b_add".to_string()));
}

#[tokio::test]
async fn an_agent_calls_an_mcp_tool() {
    let tools = stdio().connect().await.expect("connect");
    let model = ScriptedModel::new([
        vec![
            StreamEvent::ToolUse(ToolUseCall::new("call-1", "add", json!({"a": 17, "b": 25}))),
            StreamEvent::MessageStop,
        ],
        vec![
            StreamEvent::ContentDelta("17 + 25 = 42".into()),
            StreamEvent::MessageStop,
        ],
    ]);
    let mut agent =
        ConversationRuntime::new(Llm::from_provider(model), Tools::new().with_all(tools));

    let turn = agent.run_turn("What is 17 + 25?").await.expect("a turn");

    assert_eq!(turn.tool_calls[0].name, "add");
    assert_eq!(turn.tool_calls[0].output, ToolOutput::ok("42"));
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_the_tools_stops_the_server() {
    let log = CallLog::new();
    let tools = log.stdio().connect().await.expect("connect");
    let pid = log.pid();
    assert!(running(pid), "the server runs while connected");

    let boxed = llm_provider::agent::IntoTools::into_tools(tools);
    drop(boxed);

    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!running(pid), "the server process {pid} has exited");
}

/// Whether the process `pid` still runs; an exited process that nobody
/// has reaped yet (a zombie) does not.
#[cfg(unix)]
fn running(pid: u32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("run ps");
    let state = String::from_utf8_lossy(&output.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

#[cfg(unix)]
#[test]
fn dropping_the_runtime_stops_a_server_that_ignores_its_closed_input() {
    let log = CallLog::new();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let server = log.stdio().env("MCP_TEST_LINGER", "1");
    let tools = runtime.block_on(server.connect()).expect("connect");
    let pid = log.pid();
    assert!(running(pid), "the server runs while connected");

    // The tools live in a task that never ends; the runtime's teardown drops it.
    runtime.spawn(async move {
        let _tools = tools;
        std::future::pending::<()>().await;
    });
    drop(runtime);

    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!running(pid), "the server process {pid} has exited");
}

#[cfg(unix)]
#[tokio::test]
async fn a_server_that_never_answers_initialize_fails_connect_after_the_timeout() {
    let silent = McpServer::stdio("sleep", ["30"]).timeout(Duration::from_secs(1));

    let started = Instant::now();
    let refused = silent.connect().await;

    assert!(started.elapsed() < Duration::from_secs(2));
    match refused {
        Err(Error::Timeout { server, timeout }) => {
            assert_eq!(server, "sleep");
            assert_eq!(timeout, Duration::from_secs(1));
        }
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn a_tool_name_with_a_dot_is_sanitised_and_called_by_its_own_name() {
    let log = CallLog::new();
    let dotted = log.stdio().env("MCP_TEST_TOOLS", "dotted");
    let tools = dotted.connect().await.expect("connect");

    assert_eq!(tools.names(), ["dot_name"]);
    let output = call(tools, "dot_name", json!({})).await;

    assert_eq!(output.content, "dotted");
    assert_eq!(log.calls("dot.name"), 1);
}

#[tokio::test]
async fn a_prefixed_name_over_64_characters_fails_connect() {
    let prefix = "p".repeat(57);

    let refused = stdio().prefix(&prefix).connect().await;

    match refused {
        Err(Error::NameTooLong { tool, name, .. }) => {
            assert_eq!(tool, "picture");
            assert_eq!(name, format!("{prefix}_picture"));
            assert_eq!(name.len(), 65);
        }
        other => panic!("expected a name error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_blocking_approval_leaves_the_runtime_free() {
    let ticked = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&ticked);
    let tools = stdio()
        .approve(move |_, _| {
            std::thread::sleep(Duration::from_millis(200));
            seen.load(Ordering::SeqCst)
        })
        .connect()
        .await
        .expect("connect");

    // On this current-thread runtime the clock runs only while the
    // approval does not hold the thread.
    let clock = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        ticked.store(true, Ordering::SeqCst);
    });
    let output = call(tools, "echo", json!({"text": "hi"})).await;
    clock.await.expect("the clock");

    assert_eq!(output.content, "hi");
    assert!(!output.is_error);
}

#[tokio::test]
async fn http_with_the_bearer_token_lists_the_tools() {
    let url = serve_http("test-token").await;

    let tools = McpServer::http(&url)
        .bearer("test-token")
        .connect()
        .await
        .expect("connect");

    assert_eq!(sorted(tools.names()), ALL_TOOLS);
    let output = call(tools, "add", json!({"a": 2, "b": 3})).await;
    assert_eq!(output.content, "5");
}

#[tokio::test]
async fn http_without_the_bearer_token_is_an_auth_error() {
    let url = serve_http("test-token").await;

    let refused = McpServer::http(&url).connect().await;

    match refused {
        Err(Error::Auth { server }) => assert_eq!(server, url),
        other => panic!("expected an auth error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_program_that_does_not_exist_is_a_spawn_error() {
    let missing = McpServer::stdio("llm-provider-tools-no-such-server", Vec::<String>::new());

    let refused = missing.connect().await;

    assert!(
        matches!(&refused, Err(Error::Spawn { program, .. }) if program == "llm-provider-tools-no-such-server"),
        "{refused:?}"
    );
}
