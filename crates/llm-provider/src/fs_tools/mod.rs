//! Sandboxed file tools (feature `fs-tools`): [`Files`] holds `read`,
//! `write`, `edit`, `grep` and `glob`, each confined to one [`Sandbox`].
//!
//! ```no_run
//! use llm_provider::agent::Tools;
//! use llm_provider::fs_tools::{Files, Sandbox};
//!
//! # fn build() -> Result<(), Box<dyn std::error::Error>> {
//! let tools = Tools::new()
//!     .with_all(Files::new(Sandbox::new("docs")?))
//!     .only(["read", "edit"]);
//! # let _ = tools;
//! # Ok(())
//! # }
//! ```
//!
//! Every path a tool receives goes through [`Sandbox::resolve`] first,
//! which refuses one that resolves outside the root; [`Sandbox`] names what
//! that check does not cover. The tools report failures to the model as
//! the text of a failed call, from a [`ToolError`].

mod edit;
mod error;
mod files;
mod glob;
mod grep;
mod read;
mod sandbox;
mod write;

pub use error::ToolError;
pub use files::Files;
pub use sandbox::Sandbox;

#[cfg(test)]
mod tests;
