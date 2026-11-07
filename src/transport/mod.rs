//! HTTP plumbing every provider shares: the lazily built, shared HTTP client
//! with its one status check, the byte-to-line framing of event streams, the
//! idle guard and the capped body reads.
//!
//! A provider builds its request on [`Http::client`], sends it through
//! [`Http::send`] (or [`Http::byte_stream`], [`Http::json`]) and decodes what
//! comes back; nothing here knows a provider's wire format.

mod body;
mod http;
mod idle;
mod lines;

use std::pin::Pin;

use bytes::Bytes;
use tokio_stream::Stream;

use crate::provider::Error;

pub(crate) use body::{read_capped, read_text, ERROR_BODY_CAP};
pub(crate) use http::{build_client, send_checked, Http};
pub(crate) use idle::{idle_guard, within};
pub(crate) use lines::{LineAccumulator, MAX_LINE_BYTES};

/// A response body as a stream of byte chunks; the first error ends it.
pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send>>;

#[cfg(test)]
mod tests;
