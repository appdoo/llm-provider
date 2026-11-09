//! Level 1: the agent turn loop. A [`ConversationRuntime`] streams a
//! completion, runs the tools the model asks for through its [`Toolbox`],
//! feeds the results back and repeats until the model stops.
//!
//! ```no_run
//! use llm_provider::agent::{ConversationRuntime, Tools};
//! use llm_provider::OpenAiCompatible;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let model = OpenAiCompatible::new("http://localhost:11434/v1", "qwen3");
//! let mut agent = ConversationRuntime::new(model, Tools::new());
//! let turn = agent.run_turn("Say hello.").await?;
//! println!("{}", turn.response);
//! # Ok(())
//! # }
//! ```
//!
//! # Tools
//!
//! A tool is an `async fn` marked [`#[tool]`](tool): its doc comment is the
//! description the model reads, its one argument (a struct deriving
//! `Deserialize` and `JsonSchema`) the input schema, and its `Result` the
//! output (`Err` marks the call as failed). Tools that share state are
//! methods of one struct in a [`#[tools]`](tools) impl block, added with
//! [`Tools::with_all`]; [`Tools::only`] keeps the named ones. A tool whose
//! name or schema is decided at run time implements [`Tool`] by hand.
//!
//! # Agents that compose
//!
//! Two pieces make advisors, critics and delegating agents a few lines of
//! caller code. Every tool call receives the calling agent's conversation
//! as a [`Transcript`], which [`Transcript::render`] turns into text
//! another model reads as data in a user message; [`Advisor`] is such a
//! tool. And [`ConversationRuntime::into_tool`] makes an agent a tool
//! another agent calls with `{"prompt": "…"}`. A turn's
//! [`TurnSummary::tool_calls`] records every call with its input and
//! output.
//!
//! Calls to one agent run one at a time; calls to different agents run in
//! parallel, up to [`RuntimeBuilder::tool_concurrency`]. Agents in two
//! tasks talk through a channel held by a `#[tools]` block, with no new
//! type.
//!
//! # Limits
//!
//! A turn makes at most [`RuntimeBuilder::max_iterations`] model calls
//! (default 25). A tool result longer than
//! [`RuntimeBuilder::max_tool_output_chars`] (default 100 000) keeps its
//! head and tail.

mod advisor;
mod agent_tool;
mod builder;
mod error;
pub(crate) mod macro_support;
mod runtime;
#[cfg(test)]
mod test_support;
mod tool;

pub use advisor::{Advisor, CONSULT_ADVISOR};
pub use agent_tool::AgentTool;
pub use builder::RuntimeBuilder;
pub use error::RuntimeError;
pub use runtime::{ConversationRuntime, ToolCall, TurnSummary};
pub use tool::{IntoTools, Tool, ToolOutput, Toolbox, Tools, Transcript};

/// Turns an `async fn` into a [`Tool`]: the function stays as written, and a
/// unit struct named after it in CamelCase (`unix_time` → `UnixTime`)
/// implements [`Tool`].
///
/// The function takes no argument or one struct that derives `Deserialize`
/// and [`JsonSchema`](crate::JsonSchema), and returns
/// `Result<impl Display, impl Display>`. A function that also wants the
/// calling conversation names a last parameter of type
/// `&`[`Transcript<'_>`](Transcript). Its doc comment is the tool's
/// description, its name the tool's name unless `#[tool(name = "…")]` sets
/// another. A body's `Ok` value is the tool's text; its `Err` is the text of
/// a failed call, and so is an input that does not parse
/// (`invalid input: …`).
///
/// ```no_run
/// use llm_provider::agent::{tool, Tools};
///
/// /// Current time in seconds since 1970-01-01 UTC.
/// #[tool]
/// async fn unix_time() -> Result<String, std::time::SystemTimeError> {
///     let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
///     Ok(now.as_secs().to_string())
/// }
///
/// let tools = Tools::new().with(UnixTime);
/// # let _ = tools;
/// ```
pub use llm_provider_macros::tool;

/// Turns every `#[tool]` method of an impl block into a [`Tool`] and
/// implements [`IntoTools`] for the block's type, so
/// [`Tools::with_all`] adds them all.
///
/// Each tool method takes `&self` and follows the rules of [`tool`]. The
/// value is moved into one `Arc` that every tool of the block shares;
/// methods without `#[tool]` stay ordinary methods.
///
/// ```no_run
/// use llm_provider::agent::{tools, Tools};
/// use llm_provider::JsonSchema;
/// use serde::Deserialize;
///
/// /// Pages from the web, through one shared client.
/// struct Web {
///     http: reqwest::Client,
/// }
///
/// /// Which page to fetch.
/// #[derive(Deserialize, JsonSchema)]
/// struct Page {
///     /// The page's URL.
///     url: String,
/// }
///
/// #[tools]
/// impl Web {
///     /// Fetches a web page as text.
///     #[tool]
///     async fn fetch(&self, page: Page) -> Result<String, reqwest::Error> {
///         self.http.get(&page.url).send().await?.text().await
///     }
/// }
///
/// let tools = Tools::new().with_all(Web { http: reqwest::Client::new() });
/// # let _ = tools;
/// ```
pub use llm_provider_macros::tools;

#[cfg(test)]
mod tests;
