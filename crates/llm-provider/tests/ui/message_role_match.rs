use llm_provider::provider::MessageRole;

fn label(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
    }
}

fn main() {
    let _ = label(MessageRole::User);
}
