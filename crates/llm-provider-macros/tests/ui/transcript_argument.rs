use llm_provider_macros::tool;

/// Reads the conversation the old way.
#[tool]
async fn t(t: &Transcript<'_>) -> Result<String, std::io::Error> {
    Ok(t.messages.len().to_string())
}

fn main() {}
