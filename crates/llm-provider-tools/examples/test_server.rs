//! The MCP test server over stdio, for this crate's tests: `echo`, `add`,
//! `fail`, `sleep`, `env` and `picture`. Not an example to copy.
//!
//! `MCP_TEST_TOOLS=dotted` serves the one tool `dot.name` instead.
//! `MCP_TEST_LINGER` set keeps the process alive for a minute after its
//! stdin closes, as a server that ignores the end of its input would.

#[path = "../tests/support/test_server.rs"]
mod test_server;

use std::time::Duration;

use rmcp::ServiceExt;
use test_server::{record, DottedServer, TestServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    record(&format!("pid {}", std::process::id()));
    eprintln!("test server ready");

    if std::env::var_os("MCP_TEST_TOOLS").is_some_and(|tools| tools == "dotted") {
        let service = DottedServer.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
    } else {
        let service = TestServer.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
    }

    if std::env::var_os("MCP_TEST_LINGER").is_some() {
        std::thread::sleep(Duration::from_secs(60));
    }
    Ok(())
}
