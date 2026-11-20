//! Reading an AWS event stream (`application/vnd.amazon.eventstream`) as it arrives: each message is a prelude (total length,
//! headers length, the prelude's CRC), headers, a payload, and a CRC of the whole. The stream comes from a service on the bus
//! (`StartLiveTail`), but it is read as untrusted bytes all the same: every length is checked against what is there, and a
//! message that does not add up ends the stream.

use std::collections::VecDeque;

/// The most one message may be (the model's 500 events of up to 256 KB each would never fit; AWS caps a message at 16 MB).
pub const MOST: usize = 16 << 20;

/// One message: the headers whose values are strings (type 7), and the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub headers: Vec<(String, String)>,
    pub payload: Vec<u8>,
}

impl Message {
    /// The value of header `name`.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Why a stream could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Broken;

/// Bytes in, messages out.
#[derive(Debug, Default)]
pub struct Reader {
    buf: VecDeque<u8>,
}

fn be32(b: &[u8]) -> Option<usize> {
    let four: [u8; 4] = b.get(..4)?.try_into().ok()?;
    usize::try_from(u32::from_be_bytes(four)).ok()
}

// fuzz:parser eventstream
/// The headers of one message (`head` is exactly the headers' bytes). A value of a type other than a string is skipped by its
/// size, as the framing defines each.
///
/// # Errors
/// A header that runs past `head`, or of a type the framing does not have.
pub fn headers(head: &[u8]) -> Result<Vec<(String, String)>, Broken> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < head.len() {
        let n = usize::from(*head.get(at).ok_or(Broken)?);
        let name = head.get(at + 1..at + 1 + n).ok_or(Broken)?;
        at += 1 + n;
        let kind = *head.get(at).ok_or(Broken)?;
        at += 1;
        let fixed = match kind {
            0 | 1 => Some(0),
            2 => Some(1),
            3 => Some(2),
            4 => Some(4),
            5 | 8 => Some(8),
            9 => Some(16),
            6 | 7 => None,
            _ => return Err(Broken),
        };
        let size = match fixed {
            Some(size) => size,
            None => {
                let two: [u8; 2] = head
                    .get(at..at + 2)
                    .ok_or(Broken)?
                    .try_into()
                    .map_err(|_| Broken)?;
                at += 2;
                usize::from(u16::from_be_bytes(two))
            }
        };
        let value = head.get(at..at + size).ok_or(Broken)?;
        at += size;
        if kind == 7 {
            out.push((
                String::from_utf8_lossy(name).into_owned(),
                String::from_utf8_lossy(value).into_owned(),
            ));
        }
    }
    Ok(out)
}

impl Reader {
    /// Takes `bytes` and returns the messages they complete.
    ///
    /// # Errors
    /// A message whose lengths do not add up, or one larger than [`MOST`].
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Message>, Broken> {
        self.buf.extend(bytes);
        let mut out = Vec::new();
        loop {
            let prelude: Vec<u8> = self.buf.iter().take(12).copied().collect();
            if prelude.len() < 12 {
                return Ok(out);
            }
            let total = be32(&prelude).ok_or(Broken)?;
            let head = be32(prelude.get(4..).ok_or(Broken)?).ok_or(Broken)?;
            if !(16..=MOST).contains(&total) || head > total - 16 {
                return Err(Broken);
            }
            if self.buf.len() < total {
                return Ok(out);
            }
            let whole: Vec<u8> = self.buf.drain(..total).collect();
            let head_bytes = whole.get(12..12 + head).ok_or(Broken)?;
            let payload = whole.get(12 + head..total - 4).ok_or(Broken)?;
            out.push(Message {
                headers: headers(head_bytes)?,
                payload: payload.to_vec(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Broken, Reader, headers};

    #[test]
    fn messages_are_read_across_any_split() {
        let mut bytes =
            aws_wire::eventstream::event("sessionStart", "application/json", b"{\"a\":1}");
        bytes.extend(aws_wire::eventstream::exception(
            "SessionTimeoutException",
            b"{}",
        ));
        for cut in 0..bytes.len() {
            let mut r = Reader::default();
            let (a, b) = bytes.split_at(cut);
            let mut got = r.push(a).unwrap_or_default();
            got.extend(r.push(b).unwrap_or_default());
            assert_eq!(got.len(), 2, "cut at {cut}");
            assert_eq!(got[0].header(":event-type"), Some("sessionStart"));
            assert_eq!(got[0].payload, b"{\"a\":1}");
            assert_eq!(
                got[1].header(":exception-type"),
                Some("SessionTimeoutException")
            );
        }
    }

    #[test]
    fn lengths_that_do_not_add_up_end_the_stream() {
        let mut r = Reader::default();
        assert_eq!(r.push(&[0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0]), Err(Broken));
        let mut r = Reader::default();
        assert_eq!(r.push(&[0, 0, 0, 20, 0, 0, 0, 9, 0, 0, 0, 0]), Err(Broken));
        assert_eq!(headers(&[5, b'a']), Err(Broken));
        assert_eq!(headers(&[1, b'a', 42]), Err(Broken));
        // A header of another type is skipped by its size.
        assert_eq!(headers(&[1, b'a', 2, 9]), Ok(Vec::new()));
    }
}
