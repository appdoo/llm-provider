//! Store a session in a file, restore it from the file and go on; a tiny
//! compaction threshold shows one compaction with a summarizer call.
//!
//! The crate only sends events and takes a history back. The "database" here
//! is a JSON-lines file named by the session id in the current directory, written by a spawned task
//! that reads the event channel.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example session_store --features agent
//! ```
//!
//! `ANTHROPIC_BASE_URL` points it at another Anthropic-compatible endpoint;
//! with `AWS_BEARER_TOKEN_BEDROCK` set it runs on Amazon Bedrock instead:
//! Claude Opus for the session, Claude Sonnet for the compaction summaries.

use std::path::{Path, PathBuf};

use llm_provider::agent::{Session, SessionEvent, Tools};
use llm_provider::{Anthropic, Bedrock, Llm, Message, Prices};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::JoinHandle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let models = models()?;

    // Turn 1, then drop the session: only the file remains.
    let (tx, rx) = mpsc::channel(64);
    let mut agent = session(&models, tx, Vec::new());
    let path = PathBuf::from(format!("{}.jsonl", agent.session_id()));
    let writer = store(rx, path.clone(), models.prices);
    println!(
        "{}",
        agent
            .run_turn("My lucky number is 42. Remember it.")
            .await?
            .response
    );
    drop(agent);
    writer.await??;

    // Restore from the file; turn 2 refers to turn 1, turn 3 compacts first.
    let (tx, rx) = mpsc::channel(64);
    let stored = read_events(&path).await?;
    let mut agent = session(&models, tx, stored);
    let writer = store(rx, path.clone(), models.prices);
    println!(
        "{}",
        agent.run_turn("What is my lucky number?").await?.response
    );
    println!("{}", agent.run_turn("Add 8 to it.").await?.response);
    drop(agent);
    writer.await??;

    println!("stored in {}", path.display());
    Ok(())
}

/// A session on the main model that sends its events to `tx`, restored from
/// `stored` (none for a new session). With a threshold of 10 input tokens, a
/// turn that follows a model call starts by compacting its history through a
/// summary from the side model.
fn session(models: &Models, tx: mpsc::Sender<SessionEvent>, stored: Vec<SessionEvent>) -> Session {
    let summarizer = models.side.clone();
    Session::builder(models.main.clone(), Tools::new())
        .system("Answer in one short sentence.")
        .events(tx)
        .restore(stored)
        .compact_when(10, move |history: Vec<Message>| {
            let summarizer = summarizer.clone();
            async move {
                let summary = summarizer
                    .request(render(&history))
                    .system("Summarize this conversation in two sentences. Keep every number.")
                    .max_tokens(300)
                    .send()
                    .await?
                    .text;
                Ok(vec![Message::user(format!("Summary so far:\n{summary}"))])
            }
        })
        .build()
}

/// The history as plain text for the summarizer.
fn render(history: &[Message]) -> String {
    history
        .iter()
        .map(|message| format!("{:?}: {}", message.role, message.text()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Appends every event of `rx` as one JSON line to `path` and prints each
/// model call's cost; ends when the session is dropped.
fn store(
    mut rx: Receiver<SessionEvent>,
    path: PathBuf,
    prices: Prices,
) -> JoinHandle<std::io::Result<()>> {
    tokio::spawn(async move {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        while let Some(event) = rx.recv().await {
            if let SessionEvent::ModelCall { seq, usage, .. } = &event {
                println!("  call {seq}: ${:.6}", usage.cost(&prices));
            }
            if let SessionEvent::Compacted { before, after, .. } = &event {
                println!("  compacted {before} messages to {after}");
            }
            let mut line = serde_json::to_vec(&event)?;
            line.push(b'\n');
            file.write_all(&line).await?;
        }
        Ok(())
    })
}

/// The events stored in the JSON-lines file at `path`.
async fn read_events(path: &Path) -> Result<Vec<SessionEvent>, Box<dyn std::error::Error>> {
    let text = tokio::fs::read_to_string(path).await?;
    Ok(text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

/// The session's model, a side model for the summaries, and the main model's
/// prices.
struct Models {
    main: Llm,
    side: Llm,
    prices: Prices,
}

/// On Amazon Bedrock (`AWS_BEARER_TOKEN_BEDROCK`): Claude Opus for the
/// session and Claude Sonnet for the summaries. Side tasks such as a
/// compaction summary go to a smaller model, which must be a profile enabled
/// in the account and in the same geography (`eu.`); a valid id can still
/// answer 403 until the account enables the model. Regional profiles cost
/// 1.10 × the list price. Otherwise Claude Haiku for both, with
/// `ANTHROPIC_API_KEY`. The example reads the keys, the crate never does.
/// Exits 2 with a usage line when both are missing.
fn models() -> Result<Models, Box<dyn std::error::Error>> {
    if let Ok(key) = std::env::var("AWS_BEARER_TOKEN_BEDROCK") {
        let bedrock = Bedrock::new(key, "eu-central-1")?.anthropic();
        return Ok(Models {
            main: bedrock.llm(Bedrock::EU_CLAUDE_OPUS_5_5),
            side: bedrock.llm(Bedrock::EU_CLAUDE_SONNET_5_5),
            // Opus 5.5, AWS price list 2026-11-06, USD per million tokens.
            prices: Prices::per_million(4.0, 20.0)
                .cache_read(0.20)
                .regional(1.10),
        });
    }
    let key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!(
            "usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example session_store --features agent"
        );
        std::process::exit(2)
    });
    let mut builder = Anthropic::builder(key);
    if let Ok(url) = std::env::var("ANTHROPIC_BASE_URL") {
        builder = builder.base_url(url)?;
    }
    let haiku = builder.build().llm(Anthropic::CLAUDE_HAIKU_5_5);
    Ok(Models {
        main: haiku.clone(),
        side: haiku,
        prices: Prices::per_million(1.0, 5.0),
    })
}
