//! Live test of the raw cancel against a llama-server. Ignored by default;
//! it needs a llama-server started with `--parallel 1` and its origin in
//! `LLAMA_SERVER_URL` (e.g. `http://localhost:8080`), and runs with
//! `--ignored`. The variable is read here, never by the crate.

use std::time::{Duration, Instant};

use llm_provider::{OpenAi, RawRequest, StreamEvent};
use tokio_stream::StreamExt;

/// The llama-server origin from the environment.
fn origin() -> String {
    std::env::var("LLAMA_SERVER_URL").expect("LLAMA_SERVER_URL set")
}

/// True when any slot of the server at `origin` is processing.
async fn any_slot_processing(client: &reqwest::Client, origin: &str) -> bool {
    let slots: Vec<serde_json::Value> = client
        .get(format!("{origin}/slots"))
        .send()
        .await
        .expect("GET /slots")
        .json()
        .await
        .expect("slots array");
    slots
        .iter()
        .any(|slot| slot["is_processing"] == serde_json::Value::Bool(true))
}

/// L-1: dropping a raw stream after 5 tokens frees the server's slot
/// within 1 s.
#[tokio::test]
#[ignore = "live: llama-server"]
async fn dropped_raw_stream_frees_the_slot() {
    let origin = origin();
    let model = OpenAi::at(format!("{origin}/v1"))
        .expect("a valid URL")
        .raw("local");
    let request = RawRequest::new("Count from 1 to 1000, one number per line:\n").max_tokens(2000);

    let mut stream = model.stream(request);
    let mut deltas = 0;
    while deltas < 5 {
        let event = stream.next().await.expect("an event").expect("no error");
        if matches!(event, StreamEvent::ContentDelta(_)) {
            deltas += 1;
        }
    }
    drop(stream);

    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if !any_slot_processing(&client, &origin).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("a slot was still processing 1 s after the drop");
}
