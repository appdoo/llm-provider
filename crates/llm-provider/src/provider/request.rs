//! [`CompletionRequest`]: what a provider is asked to complete, with its
//! model choice, prompt-cache breakpoints and the shape of a typed answer.

use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provider::message::{Message, ToolDefinition};

/// The output token limit of a request unless one is set.
pub(crate) const DEFAULT_MAX_TOKENS: u32 = 4096;

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

impl CompletionRequest {
    /// A request to complete `messages` on the provider's model, with no
    /// system prompt, no tools, no cache breakpoints, free text and an
    /// output limit of 4096 tokens. `messages` may be an array, a `Vec` or
    /// a slice.
    ///
    /// ```no_run
    /// use llm_provider::{CompletionRequest, Message};
    ///
    /// let request = CompletionRequest::new([Message::user("hi")])
    ///     .system("Be brief.")
    ///     .max_tokens(256);
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn new(messages: impl Into<Arc<[Message]>>) -> Self {
        drop(messages);
        todo!()
    }

    /// Sets the system prompt.
    #[must_use]
    pub fn system(self, system: impl Into<String>) -> Self {
        drop(system);
        todo!()
    }

    /// Runs the request on the model with this id
    /// ([`ModelSelection::Named`]) in place of the provider's.
    #[must_use]
    pub fn model(self, model: impl Into<String>) -> Self {
        drop(model);
        todo!()
    }

    /// Sets the output token limit.
    #[must_use]
    pub fn max_tokens(self, max_tokens: u32) -> Self {
        let _ = max_tokens;
        todo!()
    }

    /// Adds tools the model may call; a further call adds more.
    #[must_use]
    pub fn tools(self, tools: impl IntoIterator<Item = ToolDefinition>) -> Self {
        drop(tools);
        todo!()
    }

    /// Adds prompt-cache breakpoints; a further call adds more.
    #[must_use]
    pub fn cache_breakpoints(
        self,
        cache_breakpoints: impl IntoIterator<Item = CacheBreakpoint>,
    ) -> Self {
        drop(cache_breakpoints);
        todo!()
    }

    /// Sets the shape the answer must take.
    #[must_use]
    pub fn response_schema(self, response_schema: ResponseSchema) -> Self {
        drop(response_schema);
        todo!()
    }
}

/// The shape of a typed answer: a name and a JSON schema. Build it with
/// [`new`](Self::new) from a schema you write, or, with the `schema`
/// feature, derive it from a Rust type with `ResponseSchema::of`.
///
/// ```
/// use llm_provider::ResponseSchema;
/// use serde_json::json;
///
/// let schema = ResponseSchema::new(
///     "Primes",
///     json!({"type": "object", "properties": {"primes": {"type": "array"}}}),
/// );
/// assert_eq!(schema.name(), "Primes");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseSchema {
    name: String,
    schema: serde_json::Value,
}

impl ResponseSchema {
    /// A shape named `name` (the response format's name on OpenAI;
    /// Anthropic's structured outputs carry no name) with the JSON schema
    /// `schema`.
    #[must_use]
    pub fn new(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
        }
    }

    /// The schema `schemars` derives for `T`, named after its title (the
    /// type name unless `#[schemars(title)]` sets another), without the
    /// `$schema` and `title` keys.
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
    #[cfg(feature = "schema")]
    #[must_use]
    pub fn of<T: JsonSchema>() -> Self {
        let (name, schema) = crate::provider::derive_schema::<T>();
        Self::new(name, schema)
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

impl CacheBreakpoint {
    /// A breakpoint that caches the prefix up to `position` for `ttl`.
    ///
    /// ```no_run
    /// use llm_provider::provider::{BreakpointPosition, CacheBreakpoint, CacheTtl};
    ///
    /// let breakpoint = CacheBreakpoint::new(BreakpointPosition::System, CacheTtl::OneHour);
    /// # let _ = breakpoint;
    /// ```
    #[must_use]
    pub fn new(position: BreakpointPosition, ttl: CacheTtl) -> Self {
        let _ = (position, ttl);
        todo!()
    }
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
