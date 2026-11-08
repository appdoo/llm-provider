use llm_provider_macros::tool;

/// Not async.
#[tool]
fn blocking() -> Result<String, std::io::Error> {
    Ok(String::new())
}

fn main() {}
