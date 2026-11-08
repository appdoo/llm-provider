//! The path sandbox: the root every file tool is confined to.
//!
//! Every file tool holds a [`Sandbox`]; every path argument goes through
//! [`Sandbox::resolve`] before any filesystem access. The resolver canonicalizes
//! the input path and fails with [`ToolError::Sandbox`] if it would escape the
//! sandbox root. For paths that do not yet exist (write/create targets), the
//! longest existing ancestor is canonicalized and the missing suffix appended;
//! dangling or looping symlinks are rejected.

use std::path::{Component, Path, PathBuf};

use super::ToolError;

/// Opt-in path-containment boundary for file-touching tools.
///
/// `Sandbox::new` canonicalizes the root at construction time (the root must
/// exist). `Sandbox::resolve` accepts an input path (absolute or relative to
/// the root) and returns a canonicalized absolute path inside the root, or a
/// [`ToolError::Sandbox`] if the path escapes.
#[derive(Debug, Clone)]
pub struct Sandbox {
    /// Canonicalized sandbox root (always an absolute, symlink-resolved path).
    root: PathBuf,
}

impl Sandbox {
    /// A sandbox rooted at `root`, which must exist.
    ///
    /// # Errors
    ///
    /// [`ToolError::Sandbox`] when `root` cannot be canonicalized.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = std::fs::canonicalize(root)
            .map_err(|e| ToolError::Sandbox(format!("canonicalize root: {e}")))?;
        Ok(Self { root })
    }

    /// The canonicalized sandbox root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve an input path against the sandbox root.
    ///
    /// Returns a canonicalized absolute path inside the root. If the path is
    /// absolute, it is used as-is (still subject to the containment check).
    /// If the path does not yet exist, its longest existing ancestor is
    /// canonicalized and the missing suffix is appended (supports write/create
    /// targets, including missing directories). A dangling or looping symlink
    /// anywhere on the path is rejected, and the missing suffix may hold only
    /// normal components (no `..` or `.`).
    ///
    /// # Errors
    ///
    /// [`ToolError::Sandbox`] on any canonicalization failure or when the
    /// resolved path does not start with the sandbox root.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, ToolError> {
        let joined = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        };

        let canon = match std::fs::canonicalize(&joined) {
            Ok(p) => p,
            Err(_) => resolve_missing(&joined)?,
        };

        if !canon.starts_with(&self.root) {
            return Err(ToolError::Sandbox(format!(
                "path {canon:?} escapes sandbox root {:?}",
                self.root
            )));
        }
        Ok(canon)
    }
}

/// Resolve a path that does not canonicalize: find its longest existing
/// ancestor, reject dangling symlinks on the way, and append the missing
/// suffix (which must hold only normal components).
fn resolve_missing(joined: &Path) -> Result<PathBuf, ToolError> {
    for candidate in joined.ancestors() {
        match std::fs::canonicalize(candidate) {
            Ok(ancestor) => {
                let suffix = joined
                    .strip_prefix(candidate)
                    .map_err(|e| ToolError::Sandbox(format!("strip prefix: {e}")))?;
                if suffix
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
                {
                    return Err(ToolError::Sandbox(format!(
                        "path component not allowed in new path: {}",
                        joined.display()
                    )));
                }
                return Ok(ancestor.join(suffix));
            }
            Err(_) if std::fs::symlink_metadata(candidate).is_ok() => {
                return Err(ToolError::Sandbox(format!(
                    "dangling symlink: {}",
                    candidate.display()
                )));
            }
            Err(_) => continue,
        }
    }
    Err(ToolError::Sandbox(format!(
        "no existing ancestor: {}",
        joined.display()
    )))
}

#[cfg(test)]
mod tests;
