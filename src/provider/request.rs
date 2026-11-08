//! [`CompletionRequest`]: what a provider is asked to complete, with its
//! model choice, prompt-cache breakpoints and the shape of a typed answer.

use std::sync::Arc;

use schemars::generate::SchemaSettings;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provider::message::{Message, ToolDefinition};

/// The model a request runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSelection {
    /// The model the provider was built with.
    Default,
    /// The model with this id.
    Named(String),
}

/// A request to complete a conversation.
///
/// `messages` is `Arc<[Message]>` so callers that keep their own history
/// clone the request without copying it; it serialises as a JSON array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// The model to run on.
    pub model: ModelSelection,
    /// The system prompt, if any.
    pub system: Option<String>,
    /// The conversation so far.
    pub messages: Arc<[Message]>,
    /// The tools the model may call.
    pub tools: Vec<ToolDefinition>,
    /// The output token limit.
    pub max_tokens: u32,
    /// Prompt-cache breakpoints; none means no caching. An empty list is
    /// left out of the JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_breakpoints: Vec<CacheBreakpoint>,
    /// The shape the answer must take; none means free text. Each provider
    /// translates it into its own wire form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<ResponseSchema>,
}

/// The shape of a typed answer: a name and a JSON schema, derived from a
/// Rust type with [`of`](Self::of).
///
/// ```
/// use llm_provider::{JsonSchema, ResponseSchema};
///
/// /// What a text is about.
/// #[derive(JsonSchema)]
/// struct Summary {
///     /// Two to four sentences.
///     sentences: Vec<String>,
/// }
///
/// let schema = ResponseSchema::of::<Summary>();
/// assert_eq!(schema.name(), "Summary");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseSchema {
    name: String,
    schema: serde_json::Value,
}

impl ResponseSchema {
    /// The schema `schemars` derives for `T`, named after its title (the
    /// type name unless `#[schemars(title)]` sets another), without the
    /// `$schema` and `title` keys.
    #[must_use]
    pub fn of<T: JsonSchema>() -> Self {
        let mut schema = SchemaSettings::draft2020_12()
            .with(|settings| settings.meta_schema = None)
            .into_generator()
            .into_root_schema_for::<T>();

        let title = schema.remove("title");
        let name = title
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| T::schema_name().into_owned(), str::to_owned);

        Self {
            name,
            schema: schema.to_value(),
        }
    }

    /// The answer's name: the type's schema title.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The answer's JSON schema.
    #[must_use]
    pub fn schema(&self) -> &serde_json::Value {
        &self.schema
    }
}

/// A prompt-cache breakpoint: cache the prefix up to and including
/// `position` for `ttl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheBreakpoint {
    /// Where the cached prefix ends.
    pub position: BreakpointPosition,
    /// How long the entry lives.
    pub ttl: CacheTtl,
}

/// Where in the request a cache breakpoint sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointPosition {
    /// After the last tool definition.
    Tools,
    /// After the system prompt.
    System,
    /// After the last content block of `messages[i]`.
    Message(usize),
}

/// The lifetime of a prompt-cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CacheTtl {
    /// Five minutes.
    FiveMinutes,
    /// One hour.
    OneHour,
}

#[cfg(test)]
mod tests;
