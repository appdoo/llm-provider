//! An own provider outside the crate, on the public core only: Ollama's
//! native `/api/generate`, NDJSON, against a local stub server.

use std::sync::{Arc, Mutex};

use llm_provider::core::{decode, Transport};
use llm_provider::provider::{
    CompletionRequest, EventStream, LlmProvider, ProviderInfo, StreamEvent,
};
use llm_provider::Llm;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Ollama {
    transport: Transport,
    model: String,
}

impl LlmProvider for Ollama {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        let prompt = request.last_user_text().unwrap_or_default();
        let body = json!({ "model": self.model, "prompt": prompt, "stream": true });
        self.transport.send_body("/api/generate", body, |bytes| {
            decode::lines(bytes, |line| {
                let v: Value = serde_json::from_str(line)?;
                Ok(v["response"].as_str().map(StreamEvent::text))
            })
        })
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo::new("ollama", &self.model)
    }
}

const NDJSON: &str = "{\"response\":\"Hel\"}\n{\"response\":\"lo\",\"done\":true}\n";

/// One request as the stub saw it: head (request line and headers) and body.
struct Seen {
    head: String,
    body: String,
}

/// A stub answering the n-th request with `statuses[n]` and the NDJSON body
/// on 200; returns its URL and the recorded requests.
async fn stub(statuses: Vec<u16>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        for status in statuses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            log.lock().unwrap().push(request);
            let body = if status == 200 { NDJSON } else { "" };
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/x-ndjson\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });
    (url, seen)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Seen {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await.unwrap();
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if n == 0 || buf.len() >= end + 4 + length {
                return Seen {
                    head: text[..end].to_string(),
                    body: text[end + 4..].to_string(),
                };
            }
        }
        assert!(n > 0, "connection closed early");
    }
}

fn llm(transport: Transport) -> Llm {
    Llm::from_provider(Ollama {
        transport,
        model: "qwen3".into(),
    })
}

/// K1: NDJSON chunks fold to "Hello"; the stub saw the exact POST.
#[tokio::test]
async fn k1_own_provider_asks_through_the_core() {
    let (url, seen) = stub(vec![200]).await;

    let answer = llm(Transport::at(&url).unwrap()).ask("hi").await.unwrap();

    assert_eq!(answer, "Hello");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].head.starts_with("POST /api/generate "),
        "{}",
        seen[0].head
    );
    let body: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(
        body,
        json!({"model": "qwen3", "prompt": "hi", "stream": true})
    );
}

/// K2: a 503 before the first content event is retried; 2 requests.
#[tokio::test]
async fn k2_retry_comes_from_the_core() {
    let (url, seen) = stub(vec![503, 200]).await;

    let answer = llm(Transport::at(&url).unwrap()).ask("hi").await.unwrap();

    assert_eq!(answer, "Hello");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

/// K3: `bearer("k")` sends `Authorization: Bearer k`.
#[tokio::test]
async fn k3_bearer_is_sent() {
    let (url, seen) = stub(vec![200]).await;

    llm(Transport::at(&url).unwrap().bearer("k"))
        .ask("hi")
        .await
        .unwrap();

    let head = seen.lock().unwrap()[0].head.to_lowercase();
    assert!(
        head.lines()
            .any(|line| line.trim() == "authorization: bearer k"),
        "{head}"
    );
}
