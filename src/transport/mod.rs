//! HTTP plumbing every provider shares: the lazily built, shared `reqwest`
//! client with its timeouts and its one status check, the byte-to-line
//! framing of event streams and the capped body reads. The idle gap is
//! `reqwest`'s read timeout, not a timer of our own.
//!
//! A provider builds its request on [`Http::client`], sends it through
//! [`Http::send`] (or [`Http::byte_stream`], [`Http::json`]) and decodes what
//! comes back; nothing here knows a provider's wire format.

mod body;
mod http;
mod lines;

use std::pin::Pin;

use bytes::Bytes;
use tokio_stream::Stream;

use crate::provider::Error;

pub(crate) use body::{read_capped, read_text, ERROR_BODY_CAP};
pub(crate) use http::{build_client, from_reqwest, Http};
pub(crate) use lines::{LineAccumulator, MAX_LINE_BYTES};

/// A response body as a stream of byte chunks; the first error ends it.
pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send>>;

#[cfg(test)]
mod tests;
