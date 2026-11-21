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
    let http = McpServer::http("https://mcp.example.com/mcp")
        .expect("a valid URL")
        .bearer("token-123");
    let stdio = McpServer::stdio("server", Vec::<String>::new()).env("GITHUB_TOKEN", "ghp-456");

    let printed = format!("{http:?} {stdio:?}");

    assert!(!printed.contains("token-123"));
    assert!(!printed.contains("ghp-456"));
}

#[test]
fn the_default_timeout_is_sixty_seconds() {
    let server = McpServer::http("https://mcp.example.com/mcp").expect("a valid URL");

    assert_eq!(server.timeout, Duration::from_secs(60));
}

#[test]
fn a_url_that_is_not_http_or_does_not_parse_is_invalid() {
    let ftp = McpServer::http("ftp://x");
    let garbage = McpServer::http("not a url");

    let Err(ftp) = ftp else {
        panic!("ftp://x was accepted")
    };
    let Err(garbage) = garbage else {
        panic!("`not a url` was accepted")
    };
    assert!(matches!(ftp, Error::InvalidUrl(_, None)), "{ftp:?}");
    assert!(
        matches!(garbage, Error::InvalidUrl(_, Some(_))),
        "{garbage:?}"
    );
    assert!(!ftp.to_string().contains("ftp://x"));
    assert!(!garbage.to_string().contains("not a url"));
}

#[tokio::test]
async fn a_bearer_over_http_to_a_remote_host_is_refused_before_any_byte() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    tokio::spawn(async move {
        while listener.accept().await.is_ok() {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });

    let refused = McpServer::http("http://h.example/mcp")
        .expect("a valid URL")
        .bearer("tok")
        .connect()
        .await;

    assert!(matches!(refused, Err(Error::InsecureBearer)), "{refused:?}");
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn m1_a_url_with_a_query_userinfo_or_fragment_is_refused_without_its_value() {
    let urls = [
        "https://h.example/mcp?key=abc",
        "https://u:p@h.example/mcp",
        "https://h.example/mcp#abc",
    ];

    for url in urls {
        let Err(error) = McpServer::http(url) else {
            panic!("{url} was accepted")
        };

        assert!(matches!(error, Error::InvalidUrl(..)), "{url}: {error:?}");
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("abc"), "abc in {text}");
            assert!(!text.contains('p'), "p in {text}");
        }
    }
}

#[test]
fn m2_a_plain_https_url_is_accepted() {
    assert!(McpServer::http("https://h.example/mcp").is_ok());
}
