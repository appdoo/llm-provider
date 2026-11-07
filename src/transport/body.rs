//! Reading response bodies to the end.

use std::time::Duration;

use crate::provider::Error;

/// The most bytes kept from the body of a non-2xx answer.
pub(crate) const ERROR_BODY_CAP: usize = 64 * 1024;

/// At most `cap` bytes of an error body as text. A read timeout is
/// [`Error::IdleTimeout`] after `idle_gap`; another broken read keeps the
/// bytes read so far; a trailing incomplete UTF-8 sequence is dropped, other
/// invalid bytes are replaced.
pub(crate) async fn read_capped(
    response: reqwest::Response,
    cap: usize,
    idle_gap: Duration,
) -> Result<String, Error> {
    todo!()
}

/// The whole body of a successful answer as text; a read timeout is
/// [`Error::IdleTimeout`] after `idle_gap`, invalid UTF-8 is
/// [`Error::InvalidUtf8`].
pub(crate) async fn read_text(
    response: reqwest::Response,
    idle_gap: Duration,
) -> Result<String, Error> {
    todo!()
}
