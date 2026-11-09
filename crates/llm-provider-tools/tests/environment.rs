//! The environment of a spawned server, in a process of its own: these
//! tests change the parent's environment, so no other test may spawn a
//! server meanwhile.

// Each test file uses only part of the shared support.
#[allow(dead_code)]
mod support;

use serde_json::json;

use support::{call, stdio};

#[tokio::test]
async fn the_server_sees_only_the_variables_it_is_given() {
    std::env::set_var("SECRET", "x");
    let server = || stdio().env("ALLOWED", "y");

    let secret = call(
        server().connect().await.expect("connect"),
        "env",
        json!({"name": "SECRET"}),
    )
    .await;
    let allowed = call(
        server().connect().await.expect("connect"),
        "env",
        json!({"name": "ALLOWED"}),
    )
    .await;

    assert_eq!(secret.content, "");
    assert_eq!(allowed.content, "y");
}
