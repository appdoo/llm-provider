use llm_provider_macros::tool;

#[tool]
async fn undocumented() -> Result<String, std::io::Error> {
    Ok(String::new())
}

fn main() {}
