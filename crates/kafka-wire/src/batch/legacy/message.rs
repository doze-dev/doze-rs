//! One message of format v0 or v1 (Kafka's `LegacyRecord`), as it lies in a message set behind its offset and size:
//!
//! ```text
//! offset i64 | size i32 | crc u32 | magic i8 | attributes i8 | [timestamp i64, v1] | key bytes | value bytes
//! ```
//!
//! `size` counts from the CRC to the end, and the CRC (CRC-32) covers the magic byte to the end. The attributes hold
//! the codec (low three bits) and, in v1, the timestamp type (bit 3). A compressed message is a wrapper: no key, and a
//! value that inflates to a message set of its own (the inner messages).

use bytes::Bytes;

use super::Magic;
use super::checksum::crc32;

/// The offset and size before each entry of a message set.
pub(super) const LOG_OVERHEAD: usize = 12;
/// The smallest message: CRC, magic, attributes and two lengths (Kafka's `RECORD_OVERHEAD_V0`).
pub(super) const OVERHEAD_V0: usize = 14;
/// Bytes through the magic byte: enough to tell an entry's format.
const THROUGH_MAGIC: usize = 17;

const CODEC_MASK: u8 = 0x07;
const LOG_APPEND_TIME: u8 = 0x08;

/// One entry of a message set: a message of format v0 or v1, or a record batch (v2), which Produce v0 to v2 may carry
/// as well.
#[derive(Debug, Clone)]
pub(super) enum Entry {
    /// A message of format v0 or v1.
    Message(Message),
    /// A v2 batch, whole.
    Batch(Bytes),
}

impl Entry {
    /// The entry's bytes, offset and size included.
    pub(super) fn bytes(&self) -> &Bytes {
        match self {
            Self::Message(m) => &m.bytes,
            Self::Batch(b) => b,
        }
    }

    /// Its format: 0, 1 or 2.
    pub(super) fn magic(&self) -> i8 {
        match self {
            Self::Message(m) => m.magic_byte(),
            Self::Batch(_) => 2,
        }
    }
}

/// Why a message set cannot be read further.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unreadable {
    /// An entry that says it is smaller than any message can be.
    Size(i32),
    /// A magic byte that is no format.
    Magic(i8),
}

/// The whole entries of a message set, in order. A last entry that is cut short is not one (Kafka reads up to it and
/// stops), and an entry that cannot be an entry at all ends the walk with an error.
pub(super) fn entries(set: &Bytes) -> impl Iterator<Item = Result<Entry, Unreadable>> + '_ {
    let mut at = 0_usize;
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let rest = set.get(at..)?;
        if rest.len() < LOG_OVERHEAD {
            return None;
        }
        let size = i32::from_be_bytes(rest.get(8..12)?.try_into().ok()?);
        if usize::try_from(size).map_or(true, |s| s < OVERHEAD_V0) {
            failed = true;
            return Some(Err(Unreadable::Size(size)));
        }
        if rest.len() < THROUGH_MAGIC {
            return None;
        }
        let magic = rest.get(16).map_or(0, |b| b.cast_signed());
        if !(0..=2).contains(&magic) {
            failed = true;
            return Some(Err(Unreadable::Magic(magic)));
        }
        let total = LOG_OVERHEAD + usize::try_from(size).ok()?;
        if rest.len() < total {
            return None;
        }
        let bytes = set.slice(at..at + total);
        at += total;
        Some(Ok(if magic == 2 {
            Entry::Batch(bytes)
        } else {
            Entry::Message(Message { bytes })
        }))
    })
}

/// A message of format v0 or v1, offset and size included.
#[derive(Debug, Clone)]
pub(super) struct Message {
    bytes: Bytes,
}

/// A message's fields do not fit its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Malformed;

impl Message {
    fn body(&self) -> &[u8] {
        self.bytes.get(LOG_OVERHEAD..).unwrap_or_default()
    }

    /// The offset written before it.
    pub(super) fn offset(&self) -> i64 {
        self.bytes
            .get(..8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map_or(0, i64::from_be_bytes)
    }

    fn magic_byte(&self) -> i8 {
        self.body().get(4).map_or(0, |b| b.cast_signed())
    }

    /// Its format.
    pub(super) fn magic(&self) -> Magic {
        if self.magic_byte() == 0 {
            Magic::V0
        } else {
            Magic::V1
        }
    }

    fn attributes(&self) -> u8 {
        self.body().get(5).copied().unwrap_or(0)
    }

    /// The codec number of its attributes (0: not compressed).
    pub(super) fn codec(&self) -> u8 {
        self.attributes() & CODEC_MASK
    }

    /// Whether it says its timestamp is the broker's (v1 only).
    pub(super) fn log_append_time(&self) -> bool {
        self.magic() == Magic::V1 && self.attributes() & LOG_APPEND_TIME != 0
    }

    /// Whether its CRC is what its contents give, and it is long enough to have every field (Kafka's
    /// `LegacyRecord.isValid`, and `ensureValid`'s size check).
    pub(super) fn is_valid(&self) -> bool {
        let body = self.body();
        let stored = body
            .get(..4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_be_bytes);
        body.len() >= self.magic().header_len()
            && stored == Some(crc32(body.get(4..).unwrap_or_default()))
    }

    /// Its timestamp: v1's field, -1 for v0, which has none.
    pub(super) fn timestamp(&self) -> i64 {
        if self.magic() == Magic::V0 {
            return -1;
        }
        self.body()
            .get(6..14)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map_or(-1, i64::from_be_bytes)
    }

    /// Its key and value, each `None` where its length is -1.
    pub(super) fn key_value(&self) -> Result<(Option<Bytes>, Option<Bytes>), Malformed> {
        let start = LOG_OVERHEAD + self.magic().header_len() - 8;
        let (key, after_key) = sized(&self.bytes, start)?;
        let (value, end) = sized(&self.bytes, after_key)?;
        if end != self.bytes.len() {
            return Err(Malformed);
        }
        Ok((key, value))
    }
}

/// A length-prefixed field of `bytes` at `at` (length -1: none), and where the next begins.
fn sized(bytes: &Bytes, at: usize) -> Result<(Option<Bytes>, usize), Malformed> {
    let len = bytes
        .get(at..at + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(i32::from_be_bytes)
        .ok_or(Malformed)?;
    if len < 0 {
        return Ok((None, at + 4));
    }
    let len = usize::try_from(len).map_err(|_| Malformed)?;
    let end = at.checked_add(4 + len).ok_or(Malformed)?;
    if end > bytes.len() {
        return Err(Malformed);
    }
    Ok((Some(bytes.slice(at + 4..end)), end))
}

impl Magic {
    /// Bytes from the CRC through the key's length: the fixed fields.
    pub(super) fn header_len(self) -> usize {
        match self {
            Self::V0 => OVERHEAD_V0,
            Self::V1 => OVERHEAD_V0 + 8,
        }
    }
}

/// What to write of one message.
#[derive(Debug, Clone, Copy)]
pub(super) struct Out<'a> {
    pub(super) magic: Magic,
    pub(super) offset: i64,
    /// Written in v1 only.
    pub(super) timestamp: i64,
    /// The timestamp type bit (v1 only).
    pub(super) log_append_time: bool,
    /// The codec number, for a wrapper; 0 otherwise.
    pub(super) codec: u8,
    pub(super) key: Option<&'a [u8]>,
    pub(super) value: Option<&'a [u8]>,
}

/// Appends one message, offset and size included, to `out`.
pub(super) fn write(out: &mut Vec<u8>, m: &Out<'_>) {
    let mut body = Vec::with_capacity(
        m.magic.header_len() + m.key.map_or(0, <[u8]>::len) + m.value.map_or(0, <[u8]>::len),
    );
    body.extend_from_slice(&[0; 4]); // the CRC, below
    let mut attributes = m.codec & CODEC_MASK;
    match m.magic {
        Magic::V0 => body.push(0),
        Magic::V1 => {
            body.push(1);
            if m.log_append_time {
                attributes |= LOG_APPEND_TIME;
            }
        }
    }
    body.push(attributes);
    if m.magic == Magic::V1 {
        body.extend_from_slice(&m.timestamp.to_be_bytes());
    }
    for field in [m.key, m.value] {
        match field {
            Some(bytes) => {
                body.extend_from_slice(
                    &i32::try_from(bytes.len()).unwrap_or(i32::MAX).to_be_bytes(),
                );
                body.extend_from_slice(bytes);
            }
            None => body.extend_from_slice(&(-1_i32).to_be_bytes()),
        }
    }
    let crc = crc32(body.get(4..).unwrap_or_default());
    if let Some(dst) = body.get_mut(..4) {
        dst.copy_from_slice(&crc.to_be_bytes());
    }
    out.extend_from_slice(&m.offset.to_be_bytes());
    out.extend_from_slice(&i32::try_from(body.len()).unwrap_or(i32::MAX).to_be_bytes());
    out.extend_from_slice(&body);
}
