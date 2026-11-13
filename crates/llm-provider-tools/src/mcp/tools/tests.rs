use serde_json::json;

use super::*;

#[test]
fn an_object_input_is_the_arguments() {
    let arguments = arguments("add", json!({"a": 2, "b": 3})).expect("an object");

    assert_eq!(
        arguments.map(serde_json::Value::Object),
        Some(json!({"a": 2, "b": 3}))
    );
}

#[test]
fn a_null_input_sends_no_arguments() {
    assert_eq!(arguments("picture", serde_json::Value::Null), Ok(None));
}

#[test]
fn any_other_input_answers_is_error() {
    let output = arguments("echo", json!("hi")).expect_err("not an object");

    assert_eq!(
        output,
        ToolOutput::error("input to echo is not a JSON object")
    );
}
