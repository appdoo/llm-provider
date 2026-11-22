//! An own provider on the public core: Ollama's native `/api/generate`,
//! NDJSON. Needs an Ollama on `localhost:11434`.
//!
//! ```sh
//! OLLAMA_MODEL=qwen3.8:27b cargo run --example own_provider
//! ```
//!
//! `OLLAMA_MODEL` names a model the server has pulled (default `qwen3`).

use llm_provider::core::{decode, Transport};
use llm_provider::provider::{
    CompletionRequest, EventStream, LlmProvider, ProviderInfo, StreamEvent,
};
use llm_provider::Llm;
use serde_json::json;

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
                let v: serde_json::Value = serde_json::from_str(line)?;
                Ok(v["response"].as_str().map(StreamEvent::text))
            })
        })
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo::new("ollama", &self.model)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = Transport::at("http://localhost:11434")?; // .bearer(key) when needed
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen3".into());

    let llm = Llm::from_provider(Ollama { transport, model });
    println!("{}", llm.ask("How many goats?").await?);
    Ok(())
}
