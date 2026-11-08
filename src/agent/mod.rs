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
mod runtime;
mod tool;

pub use builder::RuntimeBuilder;
pub use error::{RuntimeError, ToolError};
pub use runtime::{ConversationRuntime, TurnSummary};
pub use tool::{Tool, ToolOutput, Toolbox, Tools};

#[cfg(feature = "fs-tools")]
pub(crate) use tool::{optional_u64_clamped, require_string, require_string_nonempty};
