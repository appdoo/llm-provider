//! Framing a byte stream into the lines of an event stream.

use bytes::BytesMut;
use tokio_stream::StreamExt;

use crate::provider::{Error, EventStream, Oversize, StreamEvent};
use crate::transport::ByteStream;

/// The longest line accepted, in bytes.
const MAX_LINE_BYTES: usize = 1024 * 1024;
/// The buffer's starting size; one event line is usually well below it.
const INITIAL_CAPACITY: usize = 8 * 1024;

/// Splits a byte stream into `\n`-terminated lines. Bytes are buffered until
/// the `\n` arrives, so a UTF-8 sequence split across chunks is decoded
/// whole; a trailing `\r` is dropped. A line longer than [`MAX_LINE_BYTES`]
/// is [`Error::TooLarge`].
pub(crate) struct LineAccumulator {
    buf: BytesMut,
}

impl LineAccumulator {
    /// An empty accumulator.
    pub(crate) fn new() -> Self {
        Self {
            buf: BytesMut::with_capacity(INITIAL_CAPACITY),
        }
    }

    /// Appends a chunk received from the stream.
    pub(crate) fn extend(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// The next complete line without its terminator, or `None` until one is
    /// buffered. A completed line that is not UTF-8 is [`Error::InvalidUtf8`].
    pub(crate) fn next_line(&mut self) -> Result<Option<String>, Error> {
        let Some(pos) = memchr::memchr(b'\n', &self.buf) else {
            return if self.buf.len() > MAX_LINE_BYTES {
                Err(line_too_long())
            } else {
                Ok(None)
            };
        };
        if pos > MAX_LINE_BYTES {
            return Err(line_too_long());
        }

        let line = self.buf.split_to(pos + 1);
        let text = line[..pos].strip_suffix(b"\r").unwrap_or(&line[..pos]);
        Ok(Some(std::str::from_utf8(text)?.to_string()))
    }

    /// Ends the stream. An unterminated trailing fragment can never complete
    /// an event and is dropped; one that is not UTF-8 is
    /// [`Error::InvalidUtf8`].
    pub(crate) fn finish(self) -> Result<(), Error> {
        std::str::from_utf8(&self.buf)?;
        Ok(())
    }
}

/// The payload of an event stream's `data` field: `line` without its
/// `data:` prefix and one optional space after it, or `None` when `line` is
/// not a `data` line.
pub(crate) fn sse_data(line: &str) -> Option<&str> {
    let data = line.strip_prefix("data:")?;
    Some(data.strip_prefix(' ').unwrap_or(data))
}

/// A wire format's line parser, driven by [`decode_lines`].
pub(crate) trait LineDecoder {
    /// The events `line` completes, in order.
    fn feed_line(&mut self, line: &str) -> Result<Vec<StreamEvent>, Error>;
    /// True once the terminal line was seen; reading stops there.
    fn done(&self) -> bool;
    /// The events at the end of a body read to its end without `done()`.
    fn at_end(&mut self) -> Result<Vec<StreamEvent>, Error>;
}

/// Decodes a line-framed body with `decoder`. The first error ends the
/// stream. Once the decoder is [`done`](LineDecoder::done), reading stops and
/// nothing else runs. A body read to its end without it is
/// [`Error::EmptyResponse`] when it had no bytes, else it ends with the
/// decoder's [`at_end`](LineDecoder::at_end) events.
pub(crate) fn decode_lines<D: LineDecoder + Send + 'static>(
    mut bytes: ByteStream,
    mut decoder: D,
) -> EventStream {
    Box::pin(async_stream::try_stream! {
        let mut lines = LineAccumulator::new();
        let mut received = false;

        'body: while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            received |= !chunk.is_empty();
            lines.extend(&chunk);
            while let Some(line) = lines.next_line()? {
                for event in decoder.feed_line(&line)? {
                    yield event;
                }
                if decoder.done() {
                    break 'body;
                }
            }
        }

        if !decoder.done() {
            if !received {
                Err(Error::EmptyResponse)?;
            }
            lines.finish()?;
            for event in decoder.at_end()? {
                yield event;
            }
        }
    })
}

/// The error for a line over [`MAX_LINE_BYTES`].
fn line_too_long() -> Error {
    Error::TooLarge {
        what: Oversize::Line,
        max: MAX_LINE_BYTES,
    }
}

#[cfg(test)]
mod tests;
