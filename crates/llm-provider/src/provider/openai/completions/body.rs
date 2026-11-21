//! [`CompletionsBody`]: the `/completions` request body, built from a [`RawRequest`]: the prompt
//! as is, the sampling the request sets, and the caller's extra keys at the
//! top level.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::provider::error::{Error, RequestError};
use crate::provider::openai::request::StreamOptions;
use crate::provider::RawRequest;

/// The keys the body sets itself; an `extra` key among them is
/// [`RequestError::ReservedKey`](crate::provider::RequestError::ReservedKey).
const RESERVED_KEYS: [&str; 9] = [
    "model",
    "prompt",
    "max_tokens",
    "stream",
    "stream_options",
    "temperature",
    "top_p",
    "seed",
    "stop",
];

/// The streaming request body, keys in wire order.
#[derive(Debug, Serialize)]
pub(crate) struct CompletionsBody<'a> {
    model: &'a str,
    prompt: &'a str,
    max_tokens: u32,
    stream: bool,
    stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<&'a [String]>,
    #[serde(flatten)]
    extra: &'a Map<String, Value>,
}

impl<'a> CompletionsBody<'a> {
    /// The body of `request`, `model` for `ModelSelection::Default`. A
    /// non-finite sampling value is
    /// [`RequestError::NonFinite`](crate::provider::RequestError::NonFinite);
    /// the first reserved `extra` key, in the map's order, is
    /// [`RequestError::ReservedKey`](crate::provider::RequestError::ReservedKey).
    pub(crate) fn new(request: &'a RawRequest, model: &'a str) -> Result<Self, Error> {
        request.sampling.check()?;
        if let Some(key) = request
            .extra
            .keys()
            .find(|key| RESERVED_KEYS.contains(&key.as_str()))
        {
            return Err(RequestError::ReservedKey(key.clone()).into());
        }

        let sampling = &request.sampling;
        Ok(Self {
            model: request.model.resolve(model),
            prompt: &request.prompt,
            max_tokens: request.max_tokens,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
            temperature: sampling.temperature,
            top_p: sampling.top_p,
            seed: sampling.seed,
            stop: sampling.stop.as_deref(),
            extra: &request.extra,
        })
    }
}
