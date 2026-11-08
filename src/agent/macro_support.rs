//! What the code `#[tool]` and `#[tools]` emit calls: the input schema, the
//! input parse and the output conversion. Reached as
//! `llm_provider::__private`; not part of the public API.

use std::fmt::Display;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::agent::ToolOutput;

/// The JSON schema of `I`, without the `$schema` and `title` keys a tool
/// definition does not carry.
#[must_use]
pub fn input_schema<I: JsonSchema>() -> serde_json::Value {
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.meta_schema = None)
        .into_generator()
        .into_root_schema_for::<I>();
    schema.remove("title");
    schema.to_value()
}

/// The schema of a tool that takes no input: an object without properties.
#[must_use]
pub fn empty_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

/// The model's `input` parsed into an `I`.
///
/// # Errors
/// The [`ToolOutput`] the model reads when the input does not parse:
/// `invalid input: <serde message>`, marked as an error.
pub fn parse_input<I: DeserializeOwned>(input: serde_json::Value) -> Result<I, ToolOutput> {
    serde_json::from_value(input).map_err(|error| ToolOutput {
        content: format!("invalid input: {error}"),
        is_error: true,
    })
}

/// The [`ToolOutput`] of a tool body's `result`: the value's text, or the
/// error's text marked as an error.
pub fn output<T: Display, E: Display>(result: Result<T, E>) -> ToolOutput {
    match result {
        Ok(value) => ToolOutput {
            content: value.to_string(),
            is_error: false,
        },
        Err(error) => ToolOutput {
            content: error.to_string(),
            is_error: true,
        },
    }
}
