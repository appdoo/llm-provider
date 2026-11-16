use std::ffi::OsString;

use super::*;

fn variable<'a>(environment: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsString> {
    environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

#[test]
fn the_environment_keeps_only_the_inherited_variables_and_the_entries() {
    let server = McpServer::stdio("server", ["--flag"]).env("ALLOWED", "y");

    let environment = server.transport.environment();

    assert_eq!(
        variable(&environment, "ALLOWED"),
        Some(&OsString::from("y"))
    );
    assert_eq!(
        variable(&environment, "PATH"),
        std::env::var_os("PATH").as_ref()
    );
    let keys: Vec<_> = environment.iter().map(|(key, _)| key.clone()).collect();
    assert!(keys
        .iter()
        .all(|key| key == "ALLOWED" || INHERITED.iter().any(|kept| key == *kept)));
}

#[test]
fn an_entry_wins_over_an_inherited_variable() {
    let server = McpServer::stdio("server", Vec::<String>::new()).env("LANG", "C");

    let environment = server.transport.environment();

    let langs: Vec<_> = environment
        .iter()
        .filter(|(key, _)| key == "LANG")
        .collect();
    assert_eq!(langs, [&(OsString::from("LANG"), OsString::from("C"))]);
}

#[test]
fn the_server_name_is_the_program_file_name() {
    let server = McpServer::stdio("/usr/local/bin/mcp-server-fetch", ["--port", "1"]);

    assert_eq!(server.transport.name(), "mcp-server-fetch");
}

#[test]
fn debug_hides_the_bearer_token_and_env_values() {
    let http = McpServer::http("https://mcp.example.com/mcp").bearer("token-123");
    let stdio = McpServer::stdio("server", Vec::<String>::new()).env("GITHUB_TOKEN", "ghp-456");

    let printed = format!("{http:?} {stdio:?}");

    assert!(!printed.contains("token-123"));
    assert!(!printed.contains("ghp-456"));
}

#[test]
fn the_default_timeout_is_sixty_seconds() {
    let server = McpServer::http("https://mcp.example.com/mcp");

    assert_eq!(server.timeout, Duration::from_secs(60));
}
