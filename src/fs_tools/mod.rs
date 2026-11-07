//! Sandboxed file tools (feature `fs-tools`) — Read, Write, Edit, Glob and Grep behind
//! one `Sandbox`.

// Not yet restructured to the rust-design standard; pedantic applies once it is.
#![allow(clippy::pedantic)]

pub mod edit;
pub mod glob_tool;
pub mod grep_tool;
pub mod read;
mod sandbox;
pub mod write;

pub use edit::EditTool;
pub use glob_tool::GlobTool;
pub use grep_tool::GrepTool;
pub use read::ReadTool;
pub use sandbox::Sandbox;
pub use write::WriteTool;
