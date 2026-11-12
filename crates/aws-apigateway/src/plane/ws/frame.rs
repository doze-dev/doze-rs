//! WebSocket framing (RFC 6455), the server's side: client frames are masked and may be fragmented; server frames are not masked.
//! A message is capped at 1 MB (AWS's own limit is 128 KB), so a client cannot make the server hold an unbounded buffer.

use sha1::{Digest, Sha1};

/// The largest message assembled.
pub(crate) const MAX_MESSAGE: usize = 1024 * 1024;

/// What a client sent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum In {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}

/// One frame read from the front of `buf`: its FIN bit, opcode, payload, and how many bytes it took; `Ok(None)` when more bytes are
/// needed.
// fuzz:parser fuzz_ws_frame
pub(crate) fn frame(buf: &[u8]) -> Result<Option<(bool, u8, Vec<u8>, usize)>, String> {
    let (Some(&b0), Some(&b1)) = (buf.first(), buf.get(1)) else {
        return Ok(None);
    };
    let fin = b0 & 0x80 != 0;
    let opcode = b0 & 0x0f;
    let masked = b1 & 0x80 != 0;
    if !masked {
        return Err("a client frame must be masked".into());
    }
    let (len, mut at) = match b1 & 0x7f {
        126 => {
            let Some(b) = buf.get(2..4).and_then(|b| <[u8; 2]>::try_from(b).ok()) else {
                return Ok(None);
            };
            (usize::from(u16::from_be_bytes(b)), 4)
        }
        127 => {
            let Some(b) = buf.get(2..10).and_then(|b| <[u8; 8]>::try_from(b).ok()) else {
                return Ok(None);
            };
            (
                usize::try_from(u64::from_be_bytes(b)).map_err(|_| "a frame is too large")?,
                10,
            )
        }
        n => (usize::from(n), 2),
    };
    if len > MAX_MESSAGE {
        return Err("a frame is larger than 1 MB".into());
    }
    let Some(mask) = buf.get(at..at + 4) else {
        return Ok(None);
    };
    at += 4;
    let Some(data) = buf.get(at..at + len) else {
        return Ok(None);
    };
    let payload = data
        .iter()
        .zip(mask.iter().cycle())
        .map(|(b, m)| b ^ m)
        .collect();
    Ok(Some((fin, opcode, payload, at + len)))
}

/// Assembles fragmented messages.
#[derive(Debug, Default)]
pub(crate) struct Reader {
    buf: Vec<u8>,
    partial: Option<(u8, Vec<u8>)>,
}

impl Reader {
    /// Adds bytes read from the connection.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > MAX_MESSAGE * 2 {
            return Err("too much unread data".into());
        }
        Ok(())
    }

    /// The next whole message, if one has arrived.
    pub(crate) fn next(&mut self) -> Result<Option<In>, String> {
        loop {
            let Some((fin, opcode, payload, used)) = frame(&self.buf)? else {
                return Ok(None);
            };
            self.buf.drain(..used);
            match opcode {
                0x8 => return Ok(Some(In::Close)),
                0x9 => return Ok(Some(In::Ping(payload))),
                0xA => return Ok(Some(In::Pong)),
                0x1 | 0x2 if fin => return Ok(Some(message(opcode, payload))),
                0x1 | 0x2 => self.partial = Some((opcode, payload)),
                0x0 => {
                    let (op, mut so_far) = self
                        .partial
                        .take()
                        .ok_or("a continuation frame without a first frame")?;
                    so_far.extend_from_slice(&payload);
                    if so_far.len() > MAX_MESSAGE {
                        return Err("a message is larger than 1 MB".into());
                    }
                    if fin {
                        return Ok(Some(message(op, so_far)));
                    }
                    self.partial = Some((op, so_far));
                }
                other => return Err(format!("unknown opcode {other}")),
            }
        }
    }
}

fn message(opcode: u8, payload: Vec<u8>) -> In {
    if opcode == 0x1 {
        In::Text(String::from_utf8_lossy(&payload).into_owned())
    } else {
        In::Binary(payload)
    }
}

/// A server frame: text (1), binary (2), close (8), pong (10).
pub(crate) fn encode(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x80 | opcode);
    let len = payload.len();
    if len < 126 {
        out.push(u8::try_from(len).unwrap_or(0));
    } else if let Ok(short) = u16::try_from(len) {
        out.push(126);
        out.extend_from_slice(&short.to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

/// `Sec-WebSocket-Accept` for a client's `Sec-WebSocket-Key`.
pub(crate) fn accept_key(key: &str) -> String {
    let mut h = Sha1::new();
    h.update(key.trim().as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    aws_core::base64::encode(&h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mask = [1u8, 2, 3, 4];
        let mut out = vec![
            if fin { 0x80 } else { 0 } | opcode,
            0x80 | u8::try_from(payload.len()).unwrap(),
        ];
        out.extend_from_slice(&mask);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        out
    }

    #[test]
    fn the_handshake_key_is_rfc_6455s() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn fragmented_masked_messages_are_assembled() {
        let mut r = Reader::default();
        let mut bytes = masked(1, false, b"hel");
        bytes.extend(masked(0, true, b"lo"));
        bytes.extend(masked(9, true, b"p"));
        r.push(&bytes[..3]).unwrap();
        assert_eq!(r.next().unwrap(), None);
        r.push(&bytes[3..]).unwrap();
        assert_eq!(r.next().unwrap(), Some(In::Text("hello".into())));
        assert_eq!(r.next().unwrap(), Some(In::Ping(b"p".to_vec())));
        assert!(frame(&[0x81, 0x01, b'x']).is_err());
        assert_eq!(encode(1, b"hi"), vec![0x81, 2, b'h', b'i']);
    }
}
