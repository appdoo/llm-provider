//! Tests for the line framing of event streams.

use super::{LineAccumulator, MAX_LINE_BYTES};
use crate::provider::{Error, Oversize};

/// `Müller` delivered in two chunks, splitting the 2-byte `ü`
/// (0xC3 0xBC) across the chunk boundary. The accumulator must not produce
/// U+FFFD and must reconstruct the exact line.
#[test]
fn utf8_split_across_chunks_mueller() {
    let mut acc = LineAccumulator::new();
    acc.extend(&[b'M', 0xC3]);
    assert!(
        acc.next_line()
            .expect("no decode error mid-chunk")
            .is_none(),
        "no line should be available before newline arrives"
    );
    acc.extend(&[0xBC, b'l', b'l', b'e', b'r', b'\n']);
    let line = acc
        .next_line()
        .expect("must decode once complete")
        .expect("line must be available after newline");
    assert_eq!(line, "Müller");
    assert!(!line.contains('\u{FFFD}'), "must not contain U+FFFD");
}

/// The rocket emoji `🚀` (`F0 9F 9A 80`) split at each of offsets 1, 2, 3
/// still reconstructs exactly.
#[test]
fn utf8_split_four_byte_emoji_all_offsets() {
    let emoji = [0xF0, 0x9F, 0x9A, 0x80, b'\n'];
    for split in 1..=3 {
        let mut acc = LineAccumulator::new();
        acc.extend(&emoji[..split]);
        assert!(
            acc.next_line().expect("no error mid-codepoint").is_none(),
            "split={split}: no line should be available mid-codepoint"
        );
        acc.extend(&emoji[split..]);
        let line = acc
            .next_line()
            .expect("must decode once complete")
            .expect("line must be available after newline");
        assert_eq!(line, "🚀", "split at offset {split} must round-trip");
        assert!(
            !line.contains('\u{FFFD}'),
            "split={split}: must not contain U+FFFD"
        );
    }
}

/// `漢字` split inside the first codepoint.
#[test]
fn utf8_split_japanese_mid_codepoint() {
    let mut acc = LineAccumulator::new();
    acc.extend(&[0xE6]);
    assert!(acc.next_line().expect("no error").is_none());
    acc.extend(&[0xBC, 0xA2, 0xE5]);
    assert!(acc.next_line().expect("no error").is_none());
    acc.extend(&[0xAD, 0x97, b'\n']);
    let line = acc
        .next_line()
        .expect("must decode once complete")
        .expect("line must be available after newline");
    assert_eq!(line, "漢字");
    assert!(!line.contains('\u{FFFD}'), "must not contain U+FFFD");
}

/// ~100 MB of lines in 64 KiB chunks: the buffer's capacity stays bounded
/// because every completed line drains.
#[test]
fn stream_buffer_stays_bounded() {
    const CAP_BOUND: usize = 128 * 1024;
    const TOTAL: usize = 100 * 1024 * 1024;
    const LINE_LEN: usize = 1024;
    const CHUNK_LEN: usize = 64 * 1024;

    assert_eq!(CHUNK_LEN % LINE_LEN, 0);
    let mut chunk = Vec::with_capacity(CHUNK_LEN);
    for _ in 0..(CHUNK_LEN / LINE_LEN) {
        chunk.extend(std::iter::repeat_n(b'x', LINE_LEN - 1));
        chunk.push(b'\n');
    }
    assert_eq!(chunk.len(), CHUNK_LEN);

    let mut acc = LineAccumulator::new();
    let mut peak_before_drain = 0usize;
    let mut lines_drained = 0usize;
    let mut bytes_fed = 0usize;
    while bytes_fed < TOTAL {
        acc.extend(&chunk);
        bytes_fed += CHUNK_LEN;
        peak_before_drain = peak_before_drain.max(acc.buf.capacity());
        while acc.next_line().expect("well-formed stream").is_some() {
            lines_drained += 1;
        }
        assert_eq!(
            acc.buf.len(),
            0,
            "aligned chunk must fully drain to zero residual"
        );
    }

    assert_eq!(lines_drained, TOTAL / LINE_LEN);
    assert!(
        peak_before_drain <= CAP_BOUND,
        "buffer capacity must stay bounded; peak_before_drain={peak_before_drain} bound={CAP_BOUND}"
    );
}

/// `\r\n` line endings lose the `\r`.
#[test]
fn crlf_line_endings_stripped() {
    let mut acc = LineAccumulator::new();
    acc.extend(b"event: message_stop\r\n");
    let line = acc.next_line().unwrap().expect("line available");
    assert_eq!(line, "event: message_stop");
    assert!(!line.ends_with('\r'));
}

/// Closing an empty buffer is no error.
#[test]
fn empty_buffer_finish_no_error() {
    let acc = LineAccumulator::new();
    assert!(
        acc.finish().is_ok(),
        "empty buffer must close without error"
    );
}

/// A well-formed unterminated fragment is dropped at close; an invalid
/// UTF-8 fragment is an error.
#[test]
fn trailing_partial_line_close() {
    let mut acc = LineAccumulator::new();
    acc.extend(b"event: partial");
    assert!(acc.finish().is_ok());

    let mut acc = LineAccumulator::new();
    acc.extend(&[0xC3]);
    match acc.finish() {
        Err(Error::InvalidUtf8(_)) => {}
        other => panic!("expected InvalidUtf8, got {other:?}"),
    }
}

/// A 2 MiB line without newline fails with the line cap.
#[test]
fn line_cap() {
    let mut acc = LineAccumulator::new();
    acc.extend(&vec![b'x'; 2 * 1024 * 1024]);
    match acc.next_line() {
        Err(Error::TooLarge { what, max }) => {
            assert_eq!(what, Oversize::Line);
            assert_eq!(max, MAX_LINE_BYTES);
            assert_eq!(max, 1024 * 1024);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
}
