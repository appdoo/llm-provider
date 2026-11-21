//! The file tools inside a `Sandbox`, called directly: a read inside the
//! root succeeds, a read outside it fails. Runs without a network or a key.
//!
//! ```sh
//! cargo run --example sandbox --features fs-tools
//! ```

use llm_provider::agent::{CallContext, Toolbox, Tools};
use llm_provider::fs_tools::{Files, Sandbox};
use serde_json::json;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("notes.txt"), "hello sandbox\n")?;
    let elsewhere = tempfile::tempdir()?;
    let secret = elsewhere.path().join("secret.txt");
    std::fs::write(&secret, "not for the model\n")?;
    let tools = Tools::new().with_all(Files::new(Sandbox::new(root.path())?));
    let context = CallContext::default();

    let inside = tools
        .call("read", json!({ "path": "notes.txt" }), &context)
        .await;
    println!(
        "read notes.txt  -> error: {}\n{}",
        inside.is_error, inside.content
    );

    let outside = tools
        .call("read", json!({ "path": secret }), &context)
        .await;
    println!(
        "read {}  -> error: {}\n{}",
        secret.display(),
        outside.is_error,
        outside.content
    );

    if inside.is_error || !outside.is_error {
        return Err("the sandbox did not behave as expected".into());
    }
    Ok(())
}
