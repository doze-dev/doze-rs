//! The frame: `[len u32][id u32][kind u8][flags u8][method u16][body][handle trailer]`, little-endian.
//!
//! Sans-IO: [`Decoder`] takes bytes as they arrive and yields whole frames; [`encode`] makes one. `len` counts the body
//! and, when [`FLAG_HANDLE`] is set, the platform's handle trailer after it (`doze_os::channel::TRAILER_LEN` bytes, zero
//! on Unix). A frame that announces more than [`MAX_BODY`] is refused before any of it is buffered.

use thiserror::Error;

/// Bytes before the body.
pub const HEADER_LEN: usize = 12;
/// The largest body (with its trailer) a frame may carry.
pub const MAX_BODY: usize = 16 << 20;
/// Flag bit: a handle (a client connection) travels with this frame.
pub const FLAG_HANDLE: u8 = 1;

/// What a frame is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// Start of a call; `method` names it.
    Call = 1,
    /// The successful answer to a call.
    Reply = 2,
    /// A call failed; the body is an encoded error.
    Error = 3,
    /// One item of a stream.
    Item = 4,
    /// The stream is over.
    End = 5,
    /// The caller gives up on a call.
    Cancel = 6,
    /// The receiver of a stream may take this many more items (body: u32 LE).
    Credit = 7,
    /// A one-way notice from the module (health, progress).
    Event = 8,
    /// A one-way log record from the module.
    Log = 9,
}

impl Kind {
    fn from_u8(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::Call,
            2 => Kind::Reply,
            3 => Kind::Error,
            4 => Kind::Item,
            5 => Kind::End,
            6 => Kind::Cancel,
            7 => Kind::Credit,
            8 => Kind::Event,
            9 => Kind::Log,
            _ => return None,
        })
    }
}

/// One decoded frame. `trailer` is empty unless [`FLAG_HANDLE`] was set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: u32,
    pub kind: Kind,
    pub method: u16,
    pub body: Vec<u8>,
    pub handle: Option<Vec<u8>>,
}

/// Why the byte stream cannot be read any further. Every one ends the connection.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("a frame announced {0} bytes, over the {MAX_BODY}-byte limit")]
    TooLarge(usize),
    #[error("unknown frame kind {0}")]
    UnknownKind(u8),
    #[error("unknown frame flags {0:#04x}")]
    UnknownFlags(u8),
    #[error("a frame carrying a handle is shorter than the {0}-byte trailer")]
    ShortTrailer(usize),
}

/// Encodes one frame. `handle_trailer` is `Some` (possibly empty on Unix) for a frame that carries a handle.
pub fn encode(
    id: u32,
    kind: Kind,
    method: u16,
    body: &[u8],
    handle_trailer: Option<&[u8]>,
) -> Result<Vec<u8>, FrameError> {
    let trailer = handle_trailer.unwrap_or_default();
    let len = body.len() + trailer.len();
    if len > MAX_BODY {
        return Err(FrameError::TooLarge(len));
    }
    let len32 = u32::try_from(len).map_err(|_| FrameError::TooLarge(len))?;
    let mut out = Vec::with_capacity(HEADER_LEN + len);
    out.extend_from_slice(&len32.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.push(kind as u8);
    out.push(if handle_trailer.is_some() {
        FLAG_HANDLE
    } else {
        0
    });
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(trailer);
    Ok(out)
}

/// Turns a byte stream into frames.
#[derive(Debug)]
pub struct Decoder {
    buf: Vec<u8>,
    trailer_len: usize,
}

impl Decoder {
    /// A decoder for a channel whose handle trailers are `trailer_len` bytes.
    pub fn new(trailer_len: usize) -> Decoder {
        Decoder {
            buf: Vec::new(),
            trailer_len,
        }
    }

    /// Adds bytes read from the channel.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole frame, `Ok(None)` if more bytes are needed, or an error that ends the stream.
    // fuzz:parser fuzz_frame
    pub fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let Some(h) = self.buf.get(..HEADER_LEN) else {
            return Ok(None);
        };
        let field = |at: usize| {
            h.get(at..at + 4)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_le_bytes)
                .unwrap_or(0)
        };
        let len = field(0) as usize;
        if len > MAX_BODY {
            return Err(FrameError::TooLarge(len));
        }
        let id = field(4);
        let raw_kind = h.get(8).copied().unwrap_or(0);
        let kind = Kind::from_u8(raw_kind).ok_or(FrameError::UnknownKind(raw_kind))?;
        let flags = h.get(9).copied().unwrap_or(0);
        if flags & !FLAG_HANDLE != 0 {
            return Err(FrameError::UnknownFlags(flags));
        }
        let method = u16::from_le_bytes([
            h.get(10).copied().unwrap_or(0),
            h.get(11).copied().unwrap_or(0),
        ]);
        let Some(payload) = self.buf.get(HEADER_LEN..HEADER_LEN + len) else {
            return Ok(None);
        };
        let (body, handle) = if flags & FLAG_HANDLE != 0 {
            let cut = len
                .checked_sub(self.trailer_len)
                .ok_or(FrameError::ShortTrailer(self.trailer_len))?;
            let (b, t) = payload.split_at(cut);
            (b.to_vec(), Some(t.to_vec()))
        } else {
            (payload.to_vec(), None)
        };
        self.buf.drain(..HEADER_LEN + len);
        Ok(Some(Frame {
            id,
            kind,
            method,
            body,
            handle,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_any_split_of_the_byte_stream() {
        let a = encode(1, Kind::Call, 7, b"hello", None).unwrap();
        let b = encode(2, Kind::Call, 9, b"with handle", Some(&[0xAA; 4])).unwrap();
        let c = encode(3, Kind::End, 0, b"", None).unwrap();
        let stream = [a, b, c].concat();
        for cut in 0..stream.len() {
            let mut d = Decoder::new(4);
            d.push(&stream[..cut]);
            let mut got = Vec::new();
            while let Some(f) = d.next_frame().unwrap() {
                got.push(f);
            }
            d.push(&stream[cut..]);
            while let Some(f) = d.next_frame().unwrap() {
                got.push(f);
            }
            assert_eq!(got.len(), 3, "cut at {cut}");
            assert_eq!(got[1].body, b"with handle");
            assert_eq!(got[1].handle.as_deref(), Some(&[0xAA; 4][..]));
            assert_eq!((got[2].kind, got[2].body.len()), (Kind::End, 0));
        }
    }

    #[test]
    fn an_oversized_frame_is_refused_from_its_header_alone() {
        let mut d = Decoder::new(0);
        d.push(&((MAX_BODY as u32) + 1).to_le_bytes());
        d.push(&[0, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(d.next_frame(), Err(FrameError::TooLarge(MAX_BODY + 1)));
        assert!(encode(1, Kind::Call, 0, &vec![0; MAX_BODY + 1], None).is_err());
    }

    #[test]
    fn unknown_kinds_and_flags_end_the_stream() {
        let mut f = encode(1, Kind::Call, 0, b"", None).unwrap();
        f[8] = 42;
        let mut d = Decoder::new(0);
        d.push(&f);
        assert_eq!(d.next_frame(), Err(FrameError::UnknownKind(42)));
        let mut f = encode(1, Kind::Call, 0, b"", None).unwrap();
        f[9] = 0x80;
        let mut d = Decoder::new(0);
        d.push(&f);
        assert_eq!(d.next_frame(), Err(FrameError::UnknownFlags(0x80)));
    }
}
