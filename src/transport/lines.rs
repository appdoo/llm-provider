//! Framing a byte stream into the lines of an event stream.

use bytes::BytesMut;

use crate::provider::{Error, Oversize};

/// The longest line accepted, in bytes.
pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;
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

    /// The backing buffer's capacity, for the bounded-memory test.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// The bytes buffered and not yet drained.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }
}

/// The error for a line over [`MAX_LINE_BYTES`].
fn line_too_long() -> Error {
    Error::TooLarge {
        what: Oversize::Line,
        max: MAX_LINE_BYTES,
    }
}
