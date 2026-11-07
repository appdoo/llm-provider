//! Test-only local HTTP server: answers from a fixed route table, records
//! every request, and counts accepted connections. Shared by the provider
//! tests. Also holds `RecordingSleeper` for the retry tests.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::provider::retry::Sleeper;

/// `Sleeper` that records each requested duration and returns at once.
#[derive(Debug, Default)]
pub(crate) struct RecordingSleeper {
    slept: Mutex<Vec<std::time::Duration>>,
}

impl RecordingSleeper {
    /// Durations requested so far, in order.
    pub(crate) fn slept(&self) -> Vec<std::time::Duration> {
        self.slept.lock().expect("sleeper log poisoned").clone()
    }
}

#[async_trait::async_trait]
impl Sleeper for RecordingSleeper {
    async fn sleep(&self, d: std::time::Duration) {
        self.slept.lock().expect("sleeper log poisoned").push(d);
    }
}

/// A canned response: `(status, content_type, body)`.
pub(crate) type StubResponse = (u16, &'static str, String);

/// How the stub writes a reply body.
#[derive(Debug, Clone)]
pub(crate) enum StubBody {
    /// Write the body with `content-length`, then close.
    Full(String),
    /// Write the status line and headers (no `content-length`), then hold
    /// the socket open without writing a body.
    HeadersThenHang,
    /// Read the request, write nothing, hold the socket open.
    Hang,
}

/// One reply in a route's sequence.
#[derive(Debug, Clone)]
pub(crate) struct StubReply {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: StubBody,
}

impl StubReply {
    /// A `Full` reply without extra headers.
    pub(crate) fn full(status: u16, content_type: &'static str, body: &str) -> Self {
        Self {
            status,
            content_type,
            headers: Vec::new(),
            body: StubBody::Full(body.to_string()),
        }
    }
}

/// Ordered replies of one route plus the count of requests it has served.
struct Route {
    replies: Vec<StubReply>,
    served: AtomicUsize,
}

impl Route {
    /// Reply for the next request: the n-th request gets reply n, the last
    /// reply repeats.
    fn next_reply(&self) -> StubReply {
        let n = self.served.fetch_add(1, Ordering::SeqCst);
        self.replies[n.min(self.replies.len() - 1)].clone()
    }
}

/// One request as received by the stub.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordedRequest {
    pub method: String,
    pub path: String,
    /// Header names lowercased.
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Default)]
struct Shared {
    connections: AtomicUsize,
    requests: Mutex<Vec<RecordedRequest>>,
}

/// Local HTTP/1.1 stub bound on `127.0.0.1:0`. Each connection serves one
/// request and is then closed.
pub(crate) struct StubServer {
    port: u16,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl StubServer {
    /// Start a stub answering `(method, path)` from `routes`; unknown routes
    /// get a 404.
    pub(crate) async fn start(routes: Vec<((&'static str, &'static str), StubResponse)>) -> Self {
        Self::start_replies(
            routes
                .into_iter()
                .map(|(key, (status, content_type, body))| {
                    (key, vec![StubReply::full(status, content_type, &body)])
                })
                .collect(),
        )
        .await
    }

    /// Start a stub whose routes answer from ordered reply sequences.
    pub(crate) async fn start_replies(
        routes: Vec<((&'static str, &'static str), Vec<StubReply>)>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stub listener");
        let port = listener.local_addr().expect("stub local addr").port();
        let shared = Arc::new(Shared::default());
        let routes: Arc<HashMap<(String, String), Route>> = Arc::new(
            routes
                .into_iter()
                .filter(|(_, replies)| !replies.is_empty())
                .map(|((m, p), replies)| {
                    let route = Route {
                        replies,
                        served: AtomicUsize::new(0),
                    };
                    ((m.to_string(), p.to_string()), route)
                })
                .collect(),
        );

        let task_shared = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                task_shared.connections.fetch_add(1, Ordering::SeqCst);
                let shared = Arc::clone(&task_shared);
                let routes = Arc::clone(&routes);
                tokio::spawn(async move {
                    serve_one(socket, &shared, &routes).await;
                });
            }
        });

        Self { port, shared, task }
    }

    /// Base URL, e.g. `http://127.0.0.1:54321`.
    pub(crate) fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Number of TCP connections accepted so far.
    pub(crate) fn connection_count(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// All requests received so far, in arrival order.
    pub(crate) fn requests(&self) -> Vec<RecordedRequest> {
        self.shared
            .requests
            .lock()
            .expect("stub request log poisoned")
            .clone()
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_one(
    mut socket: TcpStream,
    shared: &Shared,
    routes: &HashMap<(String, String), Route>,
) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let reply = routes
        .get(&(request.method.clone(), request.path.clone()))
        .map_or_else(
            || StubReply::full(404, "text/plain", "no stub route"),
            Route::next_reply,
        );
    // Recorded as soon as it is read, before any reply is written.
    shared
        .requests
        .lock()
        .expect("stub request log poisoned")
        .push(request);

    let mut head = format!(
        "HTTP/1.1 {} STUB\r\ncontent-type: {}\r\nconnection: close\r\n",
        reply.status, reply.content_type
    );
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    match reply.body {
        StubBody::Full(body) => {
            let response = format!("{head}content-length: {}\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
        StubBody::HeadersThenHang => {
            let _ = socket.write_all(format!("{head}\r\n").as_bytes()).await;
            let _ = socket.flush().await;
            hold(socket).await;
        }
        StubBody::Hang => hold(socket).await,
    }
}

/// Keep the socket open until the stub's runtime ends.
async fn hold(socket: TcpStream) {
    std::future::pending::<()>().await;
    drop(socket);
}

/// Read one HTTP/1.1 request: head up to `\r\n\r\n`, then `content-length`
/// bytes of body.
async fn read_request(socket: &mut TcpStream) -> Option<RecordedRequest> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();

    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }

    Some(RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stub answers from its table, records method/path/lowercased
    /// headers/body, and counts one connection per request.
    #[tokio::test]
    async fn stub_serves_route_and_records_request() {
        let stub = StubServer::start(vec![(
            ("POST", "/v1/echo"),
            (201, "application/json", r#"{"ok":true}"#.to_string()),
        )])
        .await;
        assert_eq!(stub.connection_count(), 0);

        let response = reqwest::Client::new()
            .post(format!("{}/v1/echo", stub.url()))
            .header("X-Custom", "Value")
            .body("payload")
            .send()
            .await
            .expect("stub request");
        assert_eq!(response.status().as_u16(), 201);
        assert_eq!(
            response.headers()["content-type"].to_str().expect("ascii"),
            "application/json"
        );
        assert_eq!(response.text().await.expect("body"), r#"{"ok":true}"#);

        let missing = reqwest::Client::new()
            .get(format!("{}/nope", stub.url()))
            .send()
            .await
            .expect("stub request");
        assert_eq!(missing.status().as_u16(), 404);

        assert_eq!(stub.connection_count(), 2);
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/echo");
        assert_eq!(
            requests[0].headers.get("x-custom").map(String::as_str),
            Some("Value")
        );
        assert_eq!(requests[0].body, "payload");
        assert_eq!(requests[1].method, "GET");
        assert_eq!(requests[1].path, "/nope");
    }
}
