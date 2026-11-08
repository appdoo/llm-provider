//! The MCP test server over stdio, for this crate's tests: `echo`, `add`,
//! `fail`, `sleep`, `env` and `picture`. Not an example to copy.

#[path = "../tests/support/test_server.rs"]
mod test_server;

use rmcp::ServiceExt;
use test_server::{record, TestServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    record(&format!("pid {}", std::process::id()));
    eprintln!("test server ready");

    let service = TestServer.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
