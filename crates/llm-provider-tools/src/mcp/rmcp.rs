//! The adapter to the `rmcp` MCP client, the only place its types appear:
//! open a [`Session`] over stdio or Streamable HTTP, list its tools, call
//! one.

use std::process::Stdio as Pipe;

use llm_provider::provider::ToolDefinition;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ContentBlock,
    Implementation, ResourceContents, Tool,
};
use rmcp::service::{ClientInitializeError, RoleClient, RunningService, ServiceError};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::ServiceExt;
use secrecy::ExposeSecret;

use super::error::Error;
use super::reply::{Block, Reply};
use super::server::{Http, Stdio};
use super::stderr;

/// An open MCP client session; dropping it ends the session and stops a
/// spawned server.
pub(crate) struct Session {
    service: RunningService<RoleClient, ClientConfig>,
    server: String,
}

impl Session {
    /// Starts `stdio`'s program with its cleared environment, relays its
    /// stderr under `label`, and runs `initialize`.
    pub(crate) async fn spawn(stdio: &Stdio, label: String) -> Result<Self, Error> {
        let server = stdio.program().to_string_lossy().into_owned();
        let mut command = tokio::process::Command::new(stdio.program());
        command.args(stdio.args());
        command.env_clear();
        command.envs(stdio.environment());

        let spawned = TokioChildProcess::builder(command)
            .stderr(Pipe::piped())
            .spawn();
        let (transport, child_stderr) = spawned.map_err(|source| Error::Spawn {
            program: server.clone(),
            source,
        })?;
        if let Some(child_stderr) = child_stderr {
            stderr::relay(child_stderr, label);
        }

        let service = client_config().serve(transport).await;
        Self::started(service, server)
    }

    /// Opens a Streamable HTTP connection to `http`, with its bearer token,
    /// and runs `initialize`.
    pub(crate) async fn open(http: &Http) -> Result<Self, Error> {
        let mut config = StreamableHttpClientTransportConfig::with_uri(http.url());
        if let Some(token) = http.bearer() {
            config = config.auth_header(token.expose_secret());
        }

        let transport = StreamableHttpClientTransport::from_config(config);
        let service = client_config().serve(transport).await;
        Self::started(service, http.url().to_owned())
    }

    /// The session of `service` once `initialize` is done, or why it
    /// failed.
    fn started(
        service: Result<RunningService<RoleClient, ClientConfig>, ClientInitializeError>,
        server: String,
    ) -> Result<Self, Error> {
        match service {
            Ok(service) => Ok(Self { service, server }),
            Err(error) if error.is_authorization_required() => Err(Error::Auth { server }),
            Err(error) => Err(Error::Connect {
                server,
                source: Box::new(error),
            }),
        }
    }

    /// Every tool the server lists, all pages: name, description and input
    /// schema unchanged.
    pub(crate) async fn list_tools(&self) -> Result<Vec<ToolDefinition>, Error> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|error| Error::List {
                server: self.server.clone(),
                source: Box::new(error),
            })?;
        Ok(tools.into_iter().map(definition).collect())
    }

    /// Calls the server's tool `name` with `arguments`. `Err` carries a
    /// short reason: a protocol error or a lost connection.
    pub(crate) async fn call(
        &self,
        name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<Reply, String> {
        let mut params = CallToolRequestParams::new(name.to_owned());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }

        match self.service.call_tool(params).await {
            Ok(result) => Ok(reply(result)),
            Err(ServiceError::McpError(error)) => Err(error.message.into_owned()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// What this client tells a server about itself: its name and version, and
/// no capabilities.
fn client_config() -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    )
}

/// An MCP tool as the model sees it: name, description and input schema
/// unchanged.
fn definition(tool: Tool) -> ToolDefinition {
    ToolDefinition {
        name: tool.name.into_owned(),
        description: tool
            .description
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_default(),
        input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
    }
}

/// A call result in the crate's own terms.
fn reply(result: CallToolResult) -> Reply {
    Reply {
        blocks: result.content.into_iter().map(block).collect(),
        structured: result.structured_content,
        is_error: result.is_error.unwrap_or(false),
    }
}

/// One content block in the crate's own terms.
fn block(content: ContentBlock) -> Block {
    match content {
        ContentBlock::Text(text) => Block::Text(text.text),
        ContentBlock::Image(image) => Block::Image {
            mime_type: image.mime_type,
        },
        ContentBlock::Audio(audio) => Block::Audio {
            mime_type: audio.mime_type,
        },
        ContentBlock::Resource(embedded) => match embedded.resource {
            ResourceContents::TextResourceContents { uri, text, .. } => Block::Resource {
                uri,
                text: Some(text),
            },
            ResourceContents::BlobResourceContents { uri, .. } => {
                Block::Resource { uri, text: None }
            }
            _ => Block::Unsupported,
        },
        ContentBlock::ResourceLink(link) => Block::Resource {
            uri: link.uri,
            text: None,
        },
        _ => Block::Unsupported,
    }
}
