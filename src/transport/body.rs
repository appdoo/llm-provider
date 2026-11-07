//! Reading response bodies to the end.

use std::time::Duration;

use crate::provider::Error;
use crate::transport::from_reqwest;

/// The most bytes kept from the body of a non-2xx answer.
pub(crate) const ERROR_BODY_CAP: usize = 64 * 1024;

/// At most `cap` bytes of an error body as text. A read timeout is
/// [`Error::IdleTimeout`] after `idle_gap`; another broken read keeps the
/// bytes read so far; a trailing incomplete UTF-8 sequence is dropped, other
/// invalid bytes are replaced.
pub(crate) async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
    idle_gap: Duration,
) -> Result<String, Error> {
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < cap {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let room = cap - buf.len();
                buf.extend_from_slice(&chunk[..chunk.len().min(room)]);
            }
            Err(error) if error.is_timeout() => return Err(Error::IdleTimeout { after: idle_gap }),
            Ok(None) | Err(_) => break,
        }
    }
    Ok(lossy_text(buf))
}

/// `bytes` as text: a trailing incomplete UTF-8 sequence is dropped, other
/// invalid bytes are replaced.
fn lossy_text(mut bytes: Vec<u8>) -> String {
    if let Err(error) = std::str::from_utf8(&bytes) {
        if error.error_len().is_none() {
            bytes.truncate(error.valid_up_to());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The whole body of a successful answer as text; a read timeout is
/// [`Error::IdleTimeout`] after `idle_gap`, invalid UTF-8 is
/// [`Error::InvalidUtf8`].
pub(crate) async fn read_text(
    response: reqwest::Response,
    idle_gap: Duration,
) -> Result<String, Error> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| from_reqwest(error, idle_gap))?;
    Ok(std::str::from_utf8(&bytes)?.to_string())
}
