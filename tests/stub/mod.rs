//! A local HTTP server for the integration tests: answers each request with
//! the next reply of a fixed list and records the request bodies.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A running stub: its base URL and the bodies it has received.
pub struct Stub {
    url: String,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    /// A stub answering the n-th request with `replies[n]`, a
    /// `(content type, body)` pair, with status 200.
    pub async fn start(replies: Vec<(&'static str, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));

        let seen = Arc::clone(&bodies);
        tokio::spawn(async move {
            for (content_type, body) in replies {
                let (socket, _) = listener.accept().await.unwrap();
                let request = answer(socket, content_type, &body).await;
                seen.lock().unwrap().push(request);
            }
        });
        Self { url, bodies }
    }

    /// The base URL, `http://127.0.0.1:<port>`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The request bodies received so far, in order.
    #[allow(dead_code)] // Not every test target reads them.
    pub fn bodies(&self) -> Vec<String> {
        self.bodies.lock().unwrap().clone()
    }
}

/// Reads one request from `socket`, writes the reply, and returns the
/// request body.
async fn answer(mut socket: TcpStream, content_type: &str, body: &str) -> String {
    let request = read_request(&mut socket).await;
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.unwrap();
    socket.write_all(body.as_bytes()).await.unwrap();
    socket.shutdown().await.unwrap();
    request
}

/// The body of the request on `socket`, read up to its `content-length`.
async fn read_request(socket: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await.unwrap();
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        let Some(end) = text.find("\r\n\r\n") else {
            assert!(n > 0, "connection closed before the request head ended");
            continue;
        };
        let length = content_length(&text[..end]);
        if n == 0 || buf.len() >= end + 4 + length {
            return text[end + 4..].to_string();
        }
    }
}

/// The `content-length` of a request head, 0 when absent.
fn content_length(head: &str) -> usize {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0)
}
