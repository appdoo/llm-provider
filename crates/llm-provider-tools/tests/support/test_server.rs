//! The MCP test server: six tools on `rmcp`'s server side, served over
//! stdio by the `test_server` example and over Streamable HTTP in-process
//! by the tests.
//!
//! When `MCP_TEST_LOG` names a file, the server appends `pid <id>` when it
//! starts and the tool's name on every call, so a test can count calls and
//! find the process.
//!
//! [`DottedServer`] offers one tool whose name a model provider refuses,
//! `dot.name`, for the tests of name sanitising.

// rmcp's tool macros take `&self` and generate the async handler; the
// second lint is newer than the minimum toolchain.
#![allow(unknown_lints, clippy::unused_self, clippy::unused_async_trait_impl)]

use std::io::Write;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{schemars, tool, tool_router};
use serde::Deserialize;

/// The message `fail` answers with.
pub const FAIL_MESSAGE: &str = "the test server fails on purpose";

/// The test server; it keeps no state of its own.
#[derive(Debug, Clone, Default)]
pub struct TestServer;

/// The input of `echo`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Text {
    /// The text to answer with.
    text: String,
}

/// The input of `add`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Sum {
    /// The first addend.
    a: i64,
    /// The second addend.
    b: i64,
}

/// The input of `sleep`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Pause {
    /// How long to sleep, in milliseconds.
    ms: u64,
}

/// The input of `env`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Variable {
    /// The name of the environment variable.
    name: String,
}

#[tool_router(server_handler)]
impl TestServer {
    #[tool(description = "Answers with the text it is given.")]
    fn echo(&self, Parameters(Text { text }): Parameters<Text>) -> String {
        record("echo");
        text
    }

    #[tool(description = "Adds two integers.")]
    fn add(&self, Parameters(Sum { a, b }): Parameters<Sum>) -> String {
        record("add");
        (a + b).to_string()
    }

    #[tool(description = "Always fails.")]
    fn fail(&self) -> CallToolResult {
        record("fail");
        CallToolResult::error(vec![ContentBlock::text(FAIL_MESSAGE)])
    }

    #[tool(description = "Sleeps for the given milliseconds.")]
    async fn sleep(&self, Parameters(Pause { ms }): Parameters<Pause>) -> String {
        record("sleep");
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        format!("slept {ms} ms")
    }

    #[tool(description = "Reads an environment variable; empty when unset.")]
    fn env(&self, Parameters(Variable { name }): Parameters<Variable>) -> String {
        record("env");
        std::env::var(name).unwrap_or_default()
    }

    #[tool(description = "Answers with a one-pixel PNG.")]
    fn picture(&self) -> CallToolResult {
        record("picture");
        CallToolResult::success(vec![ContentBlock::image(ONE_PIXEL_PNG, "image/png")])
    }
}

/// A server with one tool, `dot.name`, whose name holds a dot.
#[allow(dead_code, reason = "served by the test_server example only")]
#[derive(Debug, Clone, Default)]
pub struct DottedServer;

#[tool_router(server_handler)]
impl DottedServer {
    #[tool(name = "dot.name", description = "Answers `dotted`.")]
    fn dot_name(&self) -> String {
        record("dot.name");
        "dotted".into()
    }
}

/// A one-pixel PNG, base64.
const ONE_PIXEL_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// Appends `event` to the file `MCP_TEST_LOG` names, if it names one.
pub fn record(event: &str) {
    let Some(path) = std::env::var_os("MCP_TEST_LOG") else {
        return;
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open MCP_TEST_LOG");
    writeln!(file, "{event}").expect("write MCP_TEST_LOG");
}
