use llm_provider_macros::tool;

/// Two arguments.
#[tool]
async fn two(first: String, second: String) -> Result<String, std::io::Error> {
    Ok(first + &second)
}

fn main() {}
