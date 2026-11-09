//! What the MCP tests share: the test server's program, its call log, the
//! in-process HTTP server, a scripted model, and a direct tool call.

pub mod test_server;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use llm_provider::agent::{IntoTools, ToolOutput, Transcript};
use llm_provider::provider::{CompletionRequest, LlmProvider, ProviderInfo, StreamEvent};
use llm_provider::EventStream;
use llm_provider_tools::mcp::{McpServer, McpTools, Stdio};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

use test_server::TestServer;

/// The test server's program, built by `cargo test` as the `test_server`
/// example next to the test binaries.
pub fn server_program() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary's path");
    let profile = exe
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target/<profile>");
    let program = profile
        .join("examples")
        .join(format!("test_server{}", std::env::consts::EXE_SUFFIX));
    assert!(
        program.exists(),
        "{} is missing: run `cargo test` (it builds the examples) or `cargo build --examples`",
        program.display()
    );
    program
}

/// The test server over stdio.
pub fn stdio() -> McpServer<Stdio> {
    McpServer::stdio(server_program(), Vec::<String>::new())
}

/// A file the test server logs its pid and its calls to.
pub struct CallLog {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl CallLog {
    /// An empty log in a fresh directory.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("calls.log");
        Self { _dir: dir, path }
    }

    /// The test server over stdio, logging to this file.
    pub fn stdio(&self) -> McpServer<Stdio> {
        stdio().env("MCP_TEST_LOG", self.path.to_string_lossy().into_owned())
    }

    /// The logged lines, oldest first.
    pub fn lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The server's pid, from its first line.
    pub fn pid(&self) -> u32 {
        let lines = self.lines();
        let pid = lines.first().and_then(|line| line.strip_prefix("pid "));
        pid.expect("the server logs its pid")
            .parse()
            .expect("a pid")
    }

    /// How many calls to `tool` the server received.
    pub fn calls(&self, tool: &str) -> usize {
        self.lines().iter().filter(|line| *line == tool).count()
    }
}

/// Serves the test server over Streamable HTTP on a local port, requiring
/// `Authorization: Bearer <token>`; the URL of its endpoint.
pub async fn serve_http(token: &'static str) -> String {
    let service = StreamableHttpService::new(
        || Ok(TestServer),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router =
        axum::Router::new()
            .nest_service("/mcp", service)
            .layer(axum::middleware::from_fn(move |request, next| {
                require_bearer(token, request, next)
            }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let address = listener.local_addr().expect("the bound address");
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{address}/mcp")
}

/// Passes `request` on when it carries the bearer `token`; else 401 with a
/// `WWW-Authenticate` challenge.
async fn require_bearer(token: &str, request: Request, next: Next) -> Response {
    let expected = format!("Bearer {token}");
    let authorization = request.headers().get(header::AUTHORIZATION);
    if authorization.and_then(|value| value.to_str().ok()) == Some(expected.as_str()) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
    )
        .into_response()
}

/// Calls the tool `name` of `tools` directly, as a runtime would.
pub async fn call(tools: McpTools, name: &str, input: serde_json::Value) -> ToolOutput {
    let tools = tools.into_tools();
    let tool = tools
        .iter()
        .find(|tool| tool.definition().name == name)
        .unwrap_or_else(|| panic!("no tool {name}"));
    tool.call(input, &Transcript::default()).await
}

/// A model that plays one event list per call.
pub struct ScriptedModel(Mutex<VecDeque<Vec<StreamEvent>>>);

impl ScriptedModel {
    /// A model that answers call `n` with the `n`-th script.
    pub fn new(scripts: impl IntoIterator<Item = Vec<StreamEvent>>) -> Self {
        Self(Mutex::new(scripts.into_iter().collect()))
    }
}

#[async_trait]
impl LlmProvider for ScriptedModel {
    fn stream(&self, _request: CompletionRequest) -> EventStream {
        let script = self.0.lock().expect("scripts").pop_front();
        let script = script.expect("no script left for this call");
        Box::pin(tokio_stream::iter(script.into_iter().map(Ok)))
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "scripted".into(),
            model: "scripted-1".into(),
        }
    }
}
