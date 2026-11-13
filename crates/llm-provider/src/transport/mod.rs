//! HTTP plumbing every provider shares: [`Http`], the lazily built `reqwest`
//! client with its timeouts and its one status check, and the line framing
//! of event streams. The idle gap is the client's read timeout.
//!
//! A provider builds its request on [`Http::client`], sends it through
//! [`Http::send`], [`Http::byte_stream`], [`Http::text`] or [`Http::json`]
//! and decodes what comes back; nothing here knows a provider's wire format.

mod body;
mod http;
mod lines;

use std::pin::Pin;

use bytes::Bytes;
use tokio_stream::Stream;

use crate::provider::Error;

pub(crate) use http::Http;
pub(crate) use lines::sse_data;
pub(crate) use lines::LineAccumulator;

/// A response body as a stream of byte chunks; the first error ends it.
pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send>>;
