//! The names a model sees for a server's tools: prefixed, with every
//! character a model provider refuses replaced, unique and at most 64
//! characters long.

use std::collections::HashMap;

use super::error::Error;

/// The longest tool name a model provider accepts.
const MAX_LEN: usize = 64;

/// The model-facing names of `remotes`, the tools of `server`, in their
/// order: `<prefix>_<remote>` when `prefix` is set, then every character
/// outside `[A-Za-z0-9_-]` replaced with `_`.
///
/// # Errors
/// [`Error::NameTooLong`] for a name over 64 characters,
/// [`Error::NameCollision`] when two tools end up with the same name.
pub(crate) fn model_names(
    server: &str,
    prefix: Option<&str>,
    remotes: &[String],
) -> Result<Vec<String>, Error> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    let mut names = Vec::with_capacity(remotes.len());
    for remote in remotes {
        let name = sanitised(&prefixed(prefix, remote));
        if name.len() > MAX_LEN {
            return Err(Error::NameTooLong {
                server: server.to_owned(),
                tool: remote.clone(),
                name,
            });
        }
        if let Some(first) = seen.insert(name.clone(), remote) {
            return Err(Error::NameCollision {
                server: server.to_owned(),
                tools: vec![first.to_owned(), remote.clone()],
                name,
            });
        }
        names.push(name);
    }
    Ok(names)
}

/// `<prefix>_<remote>` when `prefix` is set, else `remote`.
fn prefixed(prefix: Option<&str>, remote: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}_{remote}"),
        None => remote.to_owned(),
    }
}

/// `name` with every character outside `[A-Za-z0-9_-]` replaced by `_`.
fn sanitised(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
