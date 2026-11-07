//! Path sandbox — opt-in root-containment for file-touching tools.
//!
//! When a tool is configured with a [`Sandbox`], every path argument goes through
//! [`Sandbox::resolve`] before any filesystem access. The resolver canonicalizes
//! the input path and fails with [`ToolError::Sandbox`] if it would escape the
//! sandbox root. For paths that do not yet exist (write/create targets), the
//! longest existing ancestor is canonicalized and the missing suffix appended;
//! dangling or looping symlinks are rejected.

use std::path::{Component, Path, PathBuf};

use crate::agent::ToolError;

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
    /// Construct a sandbox rooted at the given path.
    ///
    /// The root must exist and be canonicalizable. Returns
    /// [`ToolError::Sandbox`] wrapping the underlying I/O error otherwise.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = std::fs::canonicalize(root)
            .map_err(|e| ToolError::Sandbox(format!("canonicalize root: {e}")))?;
        Ok(Self { root })
    }

    /// Return the canonicalized sandbox root.
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
    /// Returns [`ToolError::Sandbox`] on any canonicalization failure or if
    /// the resolved path does not start with the sandbox root.
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Sandbox rejects traversal escape `../../../etc/passwd`
    #[test]
    fn sandbox_rejects_traversal_escape() {
        let root_dir = TempDir::new().unwrap();
        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let err = sandbox.resolve("../../../etc/passwd").unwrap_err();
        match err {
            ToolError::Sandbox(msg) => {
                assert!(
                    msg.contains("escapes sandbox root") || msg.contains("canonicalize"),
                    "unexpected sandbox error: {msg}"
                );
            }
            other => panic!("expected Sandbox error, got {other:?}"),
        }
    }

    // Sandbox resolves an in-root relative path to root/path
    #[test]
    fn sandbox_resolves_in_root_path() {
        let root_dir = TempDir::new().unwrap();
        let src = root_dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("lib.rs");
        std::fs::write(&file, "// lib").unwrap();

        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let resolved = sandbox.resolve("src/lib.rs").unwrap();
        // Both sides canonicalized for robust comparison (macOS /tmp → /private/tmp).
        assert_eq!(resolved, std::fs::canonicalize(&file).unwrap());
        assert!(resolved.starts_with(sandbox.root()));
    }

    // Symlink inside sandbox pointing outside → Sandbox escape error
    #[cfg(unix)]
    #[test]
    fn symlink_escape_rejected() {
        use std::os::unix::fs::symlink;

        let outside_dir = TempDir::new().unwrap();
        let outside_file = outside_dir.path().join("secret.txt");
        std::fs::write(&outside_file, "secret").unwrap();

        let root_dir = TempDir::new().unwrap();
        let link = root_dir.path().join("escape_link");
        symlink(&outside_file, &link).unwrap();

        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let err = sandbox.resolve("escape_link").unwrap_err();
        match err {
            ToolError::Sandbox(msg) => {
                assert!(msg.contains("escapes sandbox root"), "unexpected: {msg}");
            }
            other => panic!("expected Sandbox error, got {other:?}"),
        }
    }

    // Write-target path that doesn't yet exist is resolved via parent canonicalization.
    #[test]
    fn sandbox_resolves_nonexistent_write_target() {
        let root_dir = TempDir::new().unwrap();
        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let resolved = sandbox.resolve("new_file.txt").unwrap();
        assert!(resolved.starts_with(sandbox.root()));
        assert_eq!(
            resolved.file_name().and_then(|s| s.to_str()),
            Some("new_file.txt")
        );
    }

    // Absolute path inside sandbox resolves correctly.
    #[test]
    fn sandbox_accepts_absolute_in_root() {
        let root_dir = TempDir::new().unwrap();
        let file = root_dir.path().join("x.txt");
        std::fs::write(&file, "x").unwrap();
        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let resolved = sandbox.resolve(file.to_str().unwrap()).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&file).unwrap());
    }

    // Absolute path outside sandbox is rejected.
    #[test]
    fn sandbox_rejects_absolute_outside() {
        let root_dir = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        let file = other.path().join("y.txt");
        std::fs::write(&file, "y").unwrap();
        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let err = sandbox.resolve(file.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, ToolError::Sandbox(_)));
    }

    fn sandbox_msg(err: ToolError) -> String {
        match err {
            ToolError::Sandbox(msg) => msg,
            other => panic!("expected Sandbox error, got {other:?}"),
        }
    }

    // Dangling symlink as last component → "dangling symlink: "
    #[cfg(unix)]
    #[test]
    fn dangling_symlink_rejected() {
        use std::os::unix::fs::symlink;

        let root_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        symlink(
            outside_dir.path().join("escaped.txt"),
            root_dir.path().join("link"),
        )
        .unwrap();

        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let msg = sandbox_msg(sandbox.resolve("link").unwrap_err());
        assert!(msg.starts_with("dangling symlink: "), "unexpected: {msg}");
    }

    // Self-referencing symlink loop → Sandbox error
    #[cfg(unix)]
    #[test]
    fn symlink_loop_rejected() {
        use std::os::unix::fs::symlink;

        let root_dir = TempDir::new().unwrap();
        let link = root_dir.path().join("self");
        symlink(&link, &link).unwrap();

        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let err = sandbox.resolve("self").unwrap_err();
        assert!(matches!(err, ToolError::Sandbox(_)), "got {err:?}");
    }

    // Dangling symlink as inner component → "dangling symlink: "
    #[cfg(unix)]
    #[test]
    fn dangling_inner_component_rejected() {
        use std::os::unix::fs::symlink;

        let root_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        symlink(
            outside_dir.path().join("missing_dir"),
            root_dir.path().join("dl"),
        )
        .unwrap();

        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let msg = sandbox_msg(sandbox.resolve("dl/new.txt").unwrap_err());
        assert!(msg.starts_with("dangling symlink: "), "unexpected: {msg}");
    }

    // Missing intermediate directories resolve under the root
    #[test]
    fn missing_dirs_resolve() {
        let root_dir = TempDir::new().unwrap();
        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let resolved = sandbox.resolve("a/b/c.txt").unwrap();
        let expected = std::fs::canonicalize(root_dir.path())
            .unwrap()
            .join("a/b/c.txt");
        assert_eq!(resolved, expected);
    }

    // `..` inside the missing suffix → "path component not allowed in new path: "
    #[test]
    fn parent_dir_in_missing_suffix_rejected() {
        let root_dir = TempDir::new().unwrap();
        let sandbox = Sandbox::new(root_dir.path()).unwrap();
        let msg = sandbox_msg(sandbox.resolve("a/../../x").unwrap_err());
        assert!(
            msg.starts_with("path component not allowed in new path: "),
            "unexpected: {msg}"
        );
    }
}
