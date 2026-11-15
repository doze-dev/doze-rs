//! The framing: newline-delimited JSON, one frame per line, as Go's `json.Encoder` writes it. Sans-IO: the caller
//! pushes the bytes it read and pulls whole frames, so the blocking client, a test, and (later) a tokio daemon share
//! it.
//!
//! Go's decoder would also take several values on a line or one value across lines; Go's encoder never writes either,
//! and nothing else speaks to the daemon, so a line is a frame here.

use crate::json;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The longest line read before the peer is judged broken. A large stack's status is a few hundred kilobytes.
pub const MAX_LINE: usize = 64 << 20;

/// Why a frame could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// A line grew past the limit with no newline in sight.
    #[error("a frame is longer than {0} bytes")]
    TooLong(usize),
    /// A line that is not the JSON of the type asked for.
    #[error("a frame that is not valid: {0}")]
    Json(#[from] serde_json::Error),
}

/// Encodes `v` as one line, newline included.
pub fn encode<T: Serialize + ?Sized>(v: &T) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    json::write_line(v, &mut out)?;
    Ok(out)
}

/// Splits pushed bytes into lines and decodes each.
#[derive(Debug)]
pub struct Decoder {
    buf: Vec<u8>,
    /// Where the next line starts.
    start: usize,
    /// How far past `start` has been searched for a newline already.
    scanned: usize,
    max: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Decoder::new()
    }
}

impl Decoder {
    /// A decoder with the default line limit, [`MAX_LINE`].
    pub fn new() -> Decoder {
        Decoder::with_limit(MAX_LINE)
    }

    /// A decoder that refuses lines longer than `max` bytes.
    pub fn with_limit(max: usize) -> Decoder {
        Decoder {
            buf: Vec::new(),
            start: 0,
            scanned: 0,
            max,
        }
    }

    /// Adds bytes read from the peer.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 && self.start * 2 >= self.buf.len() {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// Bytes pushed and not yet returned as a line.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start
    }

    /// The next whole line, without its newline, skipping blank ones; `None` until one has arrived.
    // fuzz:parser fuzz_codec
    pub fn next_line(&mut self) -> Result<Option<&[u8]>, CodecError> {
        let Some((begin, end)) = self.next_span()? else {
            return Ok(None);
        };
        Ok(self.buf.get(begin..end).map(<[u8]>::trim_ascii))
    }

    /// Where the next line that is not blank lies in the buffer.
    fn next_span(&mut self) -> Result<Option<(usize, usize)>, CodecError> {
        loop {
            let pending = self.buf.get(self.start..).unwrap_or_default();
            let Some(at) = pending.iter().skip(self.scanned).position(|b| *b == b'\n') else {
                self.scanned = pending.len();
                if pending.len() > self.max {
                    self.buf.clear();
                    (self.start, self.scanned) = (0, 0);
                    return Err(CodecError::TooLong(self.max));
                }
                return Ok(None);
            };
            let begin = self.start;
            let end = begin + self.scanned + at;
            (self.start, self.scanned) = (end + 1, 0);
            if end - begin > self.max {
                return Err(CodecError::TooLong(self.max));
            }
            if !self
                .buf
                .get(begin..end)
                .unwrap_or_default()
                .trim_ascii()
                .is_empty()
            {
                return Ok(Some((begin, end)));
            }
        }
    }

    /// The next frame, decoded as `T`; `None` until a whole line has arrived. A line that does not decode is
    /// consumed and returned as an error, so the caller may go on to the next.
    pub fn next_frame<T: DeserializeOwned>(&mut self) -> Result<Option<T>, CodecError> {
        match self.next_line()? {
            Some(line) => Ok(Some(serde_json::from_slice(line)?)),
            None => Ok(None),
        }
    }

    /// The bytes after the last whole line, for a reader that hands the connection on.
    pub fn into_rest(mut self) -> Decoder {
        self.buf.drain(..self.start);
        self.start = 0;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Hello};

    #[test]
    fn frames_split_anywhere_come_out_whole() {
        let bytes = b"{\"hello\":{\"proto\":2}}\n\n  \r\n{\"id\":3,\"end\":true}\r\n{\"id\":4";
        for cut in 0..bytes.len() {
            let mut d = Decoder::new();
            let mut got = Vec::new();
            for part in [bytes.get(..cut), bytes.get(cut..)].into_iter().flatten() {
                d.push(part);
                while let Ok(Some(f)) = d.next_frame::<Frame>() {
                    got.push(f);
                }
            }
            assert_eq!(got.len(), 2, "cut at {cut}");
            assert_eq!(
                got.first().and_then(|f| f.hello.clone()),
                Some(Hello {
                    proto: 2,
                    client: String::new()
                })
            );
            assert_eq!(d.buffered(), 7);
        }
    }

    #[test]
    fn a_bad_line_is_an_error_and_the_next_one_still_reads() {
        let mut d = Decoder::new();
        d.push(b"{nope}\n{\"id\":1}\n");
        assert!(matches!(d.next_frame::<Frame>(), Err(CodecError::Json(_))));
        assert!(matches!(
            d.next_frame::<Frame>(),
            Ok(Some(Frame { id: 1, .. }))
        ));
    }

    #[test]
    fn an_endless_line_is_refused() {
        let mut d = Decoder::with_limit(8);
        d.push(b"0123456789");
        assert!(matches!(d.next_line(), Err(CodecError::TooLong(8))));
        d.push(b"{}\n");
        assert!(matches!(d.next_line(), Ok(Some(b"{}"))));
        d.push(b"{\"id\":12345}\n");
        assert!(matches!(d.next_line(), Err(CodecError::TooLong(8))));
    }
}
