//! Two agents in two tasks talk through a channel. Each runtime holds a
//! `Mailbox`, a `#[tools]` block over one end of two crossed channels; no
//! new type of the crate is needed.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example mailbox --features agent
//! ```

use std::convert::Infallible;
use std::time::Duration;

use llm_provider::agent::{tools, ConversationRuntime, Tools};
use llm_provider::{Anthropic, JsonSchema};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

/// One agent's end of the conversation with the other.
struct Mailbox {
    tx: mpsc::Sender<String>,
    rx: Mutex<mpsc::Receiver<String>>,
}

/// A message for the other agent.
#[derive(Deserialize, JsonSchema)]
struct Note {
    /// The message.
    text: String,
}

#[tools]
impl Mailbox {
    /// Sends a message to the other agent.
    #[tool]
    async fn send(&self, note: Note) -> Result<String, mpsc::error::SendError<String>> {
        self.tx.send(note.text).await.map(|()| "sent".into())
    }

    /// Waits for the other agent's next message.
    #[tool]
    async fn receive(&self) -> Result<String, Infallible> {
        let next = self.rx.lock().await.recv().await;
        Ok(next.unwrap_or_else(|| "(the other agent has left)".into()))
    }
}

/// Two mailboxes whose channels cross: what one sends, the other receives.
fn pair() -> (Mailbox, Mailbox) {
    let (a_tx, b_rx) = mpsc::channel(8);
    let (b_tx, a_rx) = mpsc::channel(8);
    let a = Mailbox {
        tx: a_tx,
        rx: Mutex::new(a_rx),
    };
    let b = Mailbox {
        tx: b_tx,
        rx: Mutex::new(b_rx),
    };
    (a, b)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = api_key();
    let (a, b) = pair();
    let mut asker = ConversationRuntime::builder(
        Anthropic::new(key.as_str(), Anthropic::CLAUDE_HAIKU_5_5),
        Tools::new().with_all(a),
    )
    .system("You talk to another agent only through the send and receive tools.")
    .build();
    let mut answerer = ConversationRuntime::builder(
        Anthropic::new(key.as_str(), Anthropic::CLAUDE_HAIKU_5_5),
        Tools::new().with_all(b),
    )
    .system("You talk to another agent only through the send and receive tools.")
    .build();

    let asking = tokio::spawn(async move {
        asker
            .run_turn("Send the other agent the question: what is the capital of Australia? Then call receive once and report its answer.")
            .await
    });
    let answering = tokio::spawn(async move {
        answerer
            .run_turn("Call receive once, then send a one-sentence answer to the question you got. Then stop.")
            .await
    });

    let (asker_turn, answerer_turn) = tokio::time::timeout(Duration::from_secs(180), async {
        tokio::try_join!(asking, answering)
    })
    .await??;
    println!(
        "--- answerer ---\n{}\n\n--- asker ---\n{}",
        answerer_turn?.response, asker_turn?.response
    );
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example mailbox --features agent");
        std::process::exit(2)
    })
}
