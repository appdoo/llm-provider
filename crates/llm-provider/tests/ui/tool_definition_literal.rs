use llm_provider::ToolDefinition;

fn main() {
    let _tool = ToolDefinition {
        name: "echo".to_owned(),
        description: "Echoes its input.".to_owned(),
        input_schema: serde_json::json!({ "type": "object" }),
    };
}
