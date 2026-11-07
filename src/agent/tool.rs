//! Per-tool ToolExecutor trait, ToolOutput type, and shared input-schema helpers (llm_provider::agent::tool).

use async_trait::async_trait;

/// Output from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// The content returned by the tool.
    pub content: String,
    /// Whether the output represents an error.
    pub is_error: bool,
}

/// Trait for implementing tool execution logic.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute the tool with the given JSON input.
    async fn execute(&self, input: serde_json::Value) -> ToolOutput;
    /// The tool's name (used for registration and lookup).
    fn name(&self) -> &str;
    /// A short description of what the tool does.
    fn description(&self) -> &str;
    /// JSON Schema describing the tool's expected input.
    fn input_schema(&self) -> serde_json::Value;
}

// -------- Input-schema helpers --------
//
// These produce uniform error strings across all built-in tools. The exact
// wording is part of the public contract and is asserted by tests.

/// Require a string-typed field to be present. Returns the string slice on
/// success or a uniform error message on failure.
///
/// Error format: `"field 'X' is required and must be a string"`.
pub fn require_string<'a>(input: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    input
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("field '{field}' is required and must be a string"))
}

/// Require a string-typed field to be present and non-empty.
///
/// Error format (missing or non-string): `"field 'X' is required and must be a string"`.
/// Error format (empty string): `"field 'X' is required and must not be empty"`.
pub fn require_string_nonempty<'a>(
    input: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, String> {
    let s = require_string(input, field)?;
    if s.is_empty() {
        return Err(format!("field '{field}' is required and must not be empty"));
    }
    Ok(s)
}

/// Read an optional `u64` field, applying `default` when absent or non-numeric,
/// then clamping the result into `[min, max]`.
pub fn optional_u64_clamped(
    input: &serde_json::Value,
    field: &str,
    default: u64,
    min: u64,
    max: u64,
) -> u64 {
    input
        .get(field)
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
        .clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // require_string_nonempty on missing field → exact uniform error string.
    #[test]
    fn require_string_nonempty_missing_field() {
        let v = json!({});
        let err = require_string_nonempty(&v, "X").unwrap_err();
        assert_eq!(err, "field 'X' is required and must be a string");
    }

    // optional_u64_clamped with below-min value clamps to min.
    #[test]
    fn optional_u64_clamped_below_min() {
        let v = json!({ "t": 0 });
        assert_eq!(optional_u64_clamped(&v, "t", 30, 1, 300), 1);
    }

    // optional_u64_clamped with above-max value clamps to max.
    #[test]
    fn optional_u64_clamped_above_max() {
        let v = json!({ "t": 99_999u64 });
        assert_eq!(optional_u64_clamped(&v, "t", 30, 1, 300), 300);
    }

    // Also cover: non-empty string round-trips, missing default falls through clamp,
    // and empty-string error wording.
    #[test]
    fn require_string_nonempty_returns_value() {
        let v = json!({ "path": "/tmp/x" });
        assert_eq!(require_string_nonempty(&v, "path").unwrap(), "/tmp/x");
    }

    #[test]
    fn require_string_nonempty_empty_value() {
        let v = json!({ "path": "" });
        let err = require_string_nonempty(&v, "path").unwrap_err();
        assert_eq!(err, "field 'path' is required and must not be empty");
    }

    #[test]
    fn optional_u64_clamped_uses_default_when_absent() {
        let v = json!({});
        assert_eq!(optional_u64_clamped(&v, "t", 30, 1, 300), 30);
    }

    #[test]
    fn optional_u64_clamped_passes_through_in_range() {
        let v = json!({ "t": 42 });
        assert_eq!(optional_u64_clamped(&v, "t", 30, 1, 300), 42);
    }

    #[test]
    fn require_string_wrong_type() {
        let v = json!({ "x": 5 });
        let err = require_string(&v, "x").unwrap_err();
        assert_eq!(err, "field 'x' is required and must be a string");
    }
}
