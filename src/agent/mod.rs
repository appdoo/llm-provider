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

mod builder;
mod error;
pub(crate) mod macro_support;
mod runtime;
mod tool;

pub use builder::RuntimeBuilder;
pub use error::RuntimeError;
pub use runtime::{ConversationRuntime, TurnSummary};
pub use tool::{IntoTools, Tool, ToolOutput, Toolbox, Tools};

/// Turns an `async fn` into a [`Tool`]: the function stays as written, and a
/// unit struct named after it in CamelCase (`unix_time` → `UnixTime`)
/// implements [`Tool`].
///
/// The function takes no argument or one struct that derives `Deserialize`
/// and [`JsonSchema`](crate::JsonSchema), and returns
/// `Result<impl Display, impl Display>`. Its doc comment is the tool's
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
