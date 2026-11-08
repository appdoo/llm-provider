//! [`McpServer`]: the description of one MCP server, its transport
//! ([`Stdio`] or [`Http`]) and its options, which
//! [`connect`](McpServer::connect) turns into [`McpTools`].

use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};

use super::adapter::Session;
use super::error::Error;
use super::tools::{Approve, McpTools};

/// The variables a spawned server inherits from the caller; every other
/// variable is cleared.
const INHERITED: [&str; 4] = ["PATH", "HOME", "LANG", "TERM"];

/// How long a tool call may take unless [`McpServer::timeout`] says
/// otherwise.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// An MCP server to connect to: its transport and the options of its tools.
/// Setters return the server; [`connect`](Self::connect) opens it.
///
/// ```no_run
/// use std::time::Duration;
/// use llm_provider_tools::mcp::McpServer;
///
/// # async fn run(token: String) -> Result<(), llm_provider_tools::mcp::Error> {
/// let github = McpServer::stdio("github-mcp-server", ["stdio"])
///     .env("GITHUB_TOKEN", token)
///     .timeout(Duration::from_secs(30))
///     .prefix("github")
///     .approve(|tool, input| {
///         eprintln!("allow {tool} {input}? [y/N]");
///         false
///     })
///     .connect()
///     .await?;
/// # let _ = github;
/// # Ok(())
/// # }
/// ```
pub struct McpServer<T> {
    transport: T,
    timeout: Duration,
    prefix: Option<String>,
    approve: Option<Approve>,
}

/// A server started as a child process and spoken to over its stdin and
/// stdout.
#[derive(Debug)]
pub struct Stdio {
    program: OsString,
    args: Vec<OsString>,
    env: Vec<(String, SecretString)>,
}

/// A remote server spoken to over Streamable HTTP.
#[derive(Debug)]
pub struct Http {
    url: String,
    bearer: Option<SecretString>,
}

// Hand-written: the approval closure has no `Debug`; whether one is set
// stands for it.
impl<T: fmt::Debug> fmt::Debug for McpServer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpServer")
            .field("transport", &self.transport)
            .field("timeout", &self.timeout)
            .field("prefix", &self.prefix)
            .field("approve", &self.approve.is_some())
            .finish()
    }
}

impl<T> McpServer<T> {
    /// A server on `transport` with the default options.
    fn on(transport: T) -> Self {
        Self {
            transport,
            timeout: DEFAULT_TIMEOUT,
            prefix: None,
            approve: None,
        }
    }

    /// Sets how long one tool call may take before it answers `is_error`;
    /// 60 seconds by default.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Names every tool `<prefix>_<name>`, so that two servers offering
    /// the same tool name fit in one [`Tools`](llm_provider::agent::Tools).
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Asks `approve` before every call with the tool's name (as the model
    /// sees it) and its input; on `false` the tool answers `is_error`,
    /// `call to <name> declined`, and nothing reaches the server.
    #[must_use]
    pub fn approve(
        mut self,
        approve: impl Fn(&str, &serde_json::Value) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.approve = Some(Arc::new(approve));
        self
    }

    /// The tools of `session`, named and guarded by this server's options.
    async fn tools(self, session: Session) -> Result<McpTools, Error> {
        McpTools::list(session, self.prefix, self.timeout, self.approve).await
    }
}

impl McpServer<Stdio> {
    /// A server started as `program` with `args`. Its environment holds
    /// only `PATH`, `HOME`, `LANG`, `TERM` and the [`env`](Self::env)
    /// entries; its stderr goes to the caller's stderr, each line prefixed
    /// with the server's name.
    ///
    /// ```no_run
    /// use llm_provider_tools::mcp::McpServer;
    ///
    /// let fetch = McpServer::stdio("uvx", ["mcp-server-fetch"]);
    /// # let _ = fetch;
    /// ```
    #[must_use]
    pub fn stdio<A: Into<OsString>>(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = A>,
    ) -> Self {
        Self::on(Stdio {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
        })
    }

    /// Adds the variable `key` with `value` to the server's environment.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<SecretString>) -> Self {
        self.transport.env.push((key.into(), value.into()));
        self
    }

    /// Starts the server, runs `initialize` and lists all its tools.
    ///
    /// # Errors
    /// [`Error::Spawn`] when the program does not start, [`Error::Connect`]
    /// when the handshake fails, [`Error::List`] when listing the tools
    /// fails.
    pub async fn connect(self) -> Result<McpTools, Error> {
        let label = self.prefix.clone().unwrap_or_else(|| self.transport.name());
        let session = Session::spawn(&self.transport, label).await?;
        self.tools(session).await
    }
}

impl McpServer<Http> {
    /// A remote server at `url`, spoken to over Streamable HTTP.
    ///
    /// ```no_run
    /// use llm_provider_tools::mcp::McpServer;
    ///
    /// let docs = McpServer::http("https://mcp.example.com/mcp").bearer("token");
    /// # let _ = docs;
    /// ```
    #[must_use]
    pub fn http(url: impl Into<String>) -> Self {
        Self::on(Http {
            url: url.into(),
            bearer: None,
        })
    }

    /// Sends `token` as `Authorization: Bearer <token>` with every request.
    #[must_use]
    pub fn bearer(mut self, token: impl Into<SecretString>) -> Self {
        self.transport.bearer = Some(token.into());
        self
    }

    /// Opens the connection, runs `initialize` and lists all its tools.
    ///
    /// # Errors
    /// [`Error::Auth`] when the server asks for credentials,
    /// [`Error::Connect`] when the connection or the handshake fails,
    /// [`Error::List`] when listing the tools fails.
    pub async fn connect(self) -> Result<McpTools, Error> {
        let session = Session::open(&self.transport).await?;
        self.tools(session).await
    }
}

impl Stdio {
    /// The program to start.
    pub(crate) fn program(&self) -> &OsString {
        &self.program
    }

    /// The program's arguments.
    pub(crate) fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The server's name in stderr lines and errors: the program's file
    /// name.
    pub(crate) fn name(&self) -> String {
        let program = Path::new(&self.program);
        program
            .file_name()
            .unwrap_or(program.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// The child's whole environment: the [`INHERITED`] variables the
    /// caller has, then the `env` entries, which win.
    pub(crate) fn environment(&self) -> Vec<(OsString, OsString)> {
        let entries = self.env.iter().map(|(key, value)| {
            let value = OsString::from(value.expose_secret());
            (OsString::from(key), value)
        });
        let inherited = INHERITED
            .iter()
            .filter(|key| !self.env.iter().any(|(entry, _)| entry == *key))
            .filter_map(|key| Some((OsString::from(key), std::env::var_os(key)?)));
        inherited.chain(entries).collect()
    }
}

impl Http {
    /// The server's URL.
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    /// The bearer token, if one is set.
    pub(crate) fn bearer(&self) -> Option<&SecretString> {
        self.bearer.as_ref()
    }
}

#[cfg(test)]
mod tests;
