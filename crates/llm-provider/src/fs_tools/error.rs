//! [`ToolError`]: why a file tool failed, and the text the model reads.

use std::path::PathBuf;

/// Why a file tool failed. Its message is the text the model reads.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolError {
    /// The input did not parse into the tool's input type.
    #[error("invalid input: {0}")]
    InvalidInput(#[from] serde_json::Error),
    /// A path escapes the sandbox root, or cannot be resolved inside it.
    #[error("Sandbox error: {0}")]
    Sandbox(String),
    /// A file over the read cap was read without `offset` or `limit`.
    #[error("Input too large: {0}")]
    InputTooLarge(String),
    /// The path names a directory, device or other non-regular file.
    #[error("not a regular file: {0}")]
    NotAFile(String),
    /// Reading or writing `path` failed.
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        /// The file or directory the operation touched.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Edit's `old_string` does not occur in the file.
    #[error("string not found in {}", .0.display())]
    NotFound(PathBuf),
    /// Edit's `old_string` occurs more than once in the file.
    #[error(
        "string appears {count} times in {} — provide more context to make it \
         unique",
        path.display()
    )]
    NotUnique {
        /// How often the string occurs.
        count: usize,
        /// The file searched.
        path: PathBuf,
    },
    /// Grep's `pattern` is not a valid regular expression.
    #[error("invalid regex: {0}")]
    Regex(#[from] regex::Error),
    /// Glob's `pattern` is not a valid glob.
    #[error("invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),
    /// The tool's file work panicked.
    #[error("the file tool panicked")]
    Panicked(#[source] tokio::task::JoinError),
}

impl ToolError {
    /// A closure that wraps an I/O error on `path`, for `map_err`.
    pub(super) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}
