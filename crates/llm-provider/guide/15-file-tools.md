Five file tools for an agent, `read`, `write`, `edit`, `grep` and `glob`,
each confined to one directory. Needs the `fs-tools` feature.

# The call

[`Files::new(sandbox)`](crate::fs_tools::Files::new) holds the five tools;
[`Sandbox::new(root)`](crate::fs_tools::Sandbox::new) names the directory,
which must exist. `Files` is a `#[tools]` block, so
[`Tools::with_all`](crate::agent::Tools::with_all) adds it and
[`Tools::only`](crate::agent::Tools::only) keeps the tools you name.

```rust
use llm_provider::agent::Tools;
use llm_provider::fs_tools::{Files, Sandbox};

let root = tempfile::tempdir()?;

let reviewer = Tools::new()
    .with_all(Files::new(Sandbox::new(root.path())?))
    .only(["read", "grep", "glob"]);
# let _ = reviewer;
# Ok::<(), Box<dyn std::error::Error>>(())
```

# What the model sends

- `read`: `path`, and optionally `offset` (the first line, from 0) and
  `limit` (2000 lines when unset). A file over 10 MiB needs one of the two.
- `write`: `path` and `content`; creates or overwrites the file and its
  parent directories.
- `edit`: `path`, `old_string` and `new_string`; `old_string` must occur
  exactly once.
- `grep`: a regular expression `pattern`, optionally a `path` (the root when
  unset) and `context` lines around each match. The walk skips hidden
  entries and does not follow symlinks.
- `glob`: a `pattern`, optionally a base `path`.

# The sandbox

Every path a tool receives goes through
[`Sandbox::resolve`](crate::fs_tools::Sandbox::resolve) first. It takes a
path absolute or relative to the root, resolves symlinks, and refuses one
that ends outside the root. A path that does not exist yet (a file to
write) resolves through its longest existing ancestor; its missing part may
hold no `..` or `.`, and a dangling or looping symlink is refused.

The sandbox does not cover the race between resolving a path and using it
(another process can swap a path component in between), and it does not
detect hard links, which point into the root from anywhere. Run the tools
on a directory no other process writes to.

# Errors

A tool that fails answers the model with an error result, from a
[`ToolError`](crate::fs_tools::ToolError); the turn goes on
([chapter 9](crate::guide::ch09_agent_loop)). The text the model reads:

- `Sandbox error: …`: the path escapes the root or cannot be resolved;
- `Input too large: …`: a file over 10 MiB read without `offset` or `limit`;
- `not a regular file: …`: a directory or a device;
- `string not found in …` and `string appears N times in …`: an `edit`
  whose `old_string` is missing or not unique;
- `invalid regex: …` and `invalid glob pattern: …`;
- `I/O error on …` and `invalid input: …`.

The tools can be called without a model, through
[`Toolbox::call`](crate::agent::Toolbox::call), as a test does:

```rust,no_run
use llm_provider::agent::{CallContext, Toolbox, Tools};
use llm_provider::fs_tools::{Files, Sandbox};
use serde_json::json;

async fn outside(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let tools = Tools::new().with_all(Files::new(Sandbox::new(root)?));

    let output = tools
        .call("read", json!({ "path": "/etc/hosts" }), &CallContext::default())
        .await;

    assert!(output.is_error);
    println!("{}", output.content);
    Ok(())
}
```

# Example

[`examples/sandbox.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/sandbox.rs):
the file tools called directly; a read outside the root fails. It runs
without a network or a key.
[`examples/file_agent.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/file_agent.rs):
a local model that fixes typos with the file tools.

```sh
cargo run --example sandbox --features fs-tools
```
