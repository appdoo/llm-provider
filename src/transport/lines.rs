//! Framing a byte stream into the lines of an event stream.

use bytes::BytesMut;

use crate::provider::Error;

/// The longest line accepted, in bytes.
pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;

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
        todo!()
    }

    /// Appends a chunk received from the stream.
    pub(crate) fn extend(&mut self, chunk: &[u8]) {
        todo!()
    }

    /// The next complete line without its terminator, or `None` until one is
    /// buffered. A completed line that is not UTF-8 is [`Error::InvalidUtf8`].
    pub(crate) fn next_line(&mut self) -> Result<Option<String>, Error> {
        todo!()
    }

    /// Ends the stream. An unterminated trailing fragment can never complete
    /// an event and is dropped; one that is not UTF-8 is
    /// [`Error::InvalidUtf8`].
    pub(crate) fn finish(self) -> Result<(), Error> {
        todo!()
    }
}
