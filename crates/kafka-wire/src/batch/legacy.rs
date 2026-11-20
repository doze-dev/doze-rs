//! Message formats v0 and v1: what Kafka stored before record batches (KIP-98), and what Produce v0 to v2 may carry and
//! Fetch v0 to v3 read. The log keeps record batch format v2 only, so the broker converts, as Kafka does when a topic's
//! message format is newer than a client's:
//!
//! - [`up`]: a Produce v0 to v2 message set becomes one v2 batch, on the way in (Kafka's `LogValidator`: offsets are the
//!   broker's, a v0 message has no timestamp (-1), a v1 message keeps its own, a wrapper's inner messages are taken out
//!   of their codec and the batch is written in the topic's).
//! - [`down`]: the v2 batches a Fetch v0 to v3 read become messages of format v0 (Fetch v0 and v1) or v1 (v2 and v3), on
//!   the way out (Kafka's `RecordsUtil.downConvert`, chunked as `LazyDownConversionRecords` from Kafka 2.0, KIP-283):
//!   control batches are dropped, headers are lost, a compressed batch becomes one wrapper in its codec.
//!
//! Neither path is taken by a client of Produce v3 or Fetch v4 and later, which are the batches as they are.

mod checksum;
mod codec;
mod down;
mod message;
mod records;
mod up;

use bytes::{BufMut, Bytes, BytesMut};

pub use down::{DownError, DownRules, Downconversion, down};
pub use up::{Up, UpError, UpRules, up};

use super::layout::{CRC, LENGTH, crc_of};
use super::varint;

/// A message format before record batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Magic {
    /// No timestamps (Kafka before 0.10).
    V0,
    /// A timestamp and its type in each message (KIP-32).
    V1,
}

/// One record on its way between formats.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rec {
    offset: i64,
    timestamp: i64,
    key: Option<Bytes>,
    value: Option<Bytes>,
    headers: Vec<(Bytes, Bytes)>,
}

/// One uncompressed v2 batch of `recs`, as Kafka's `MemoryRecordsBuilder` writes one for a client: base offset 0 (the log
/// gives the offset), no producer, CreateTime, the first record's timestamp as the batch's first and the newest as its
/// newest (-1 for records that have none).
fn v2_batch(recs: &[Rec]) -> Bytes {
    let first = recs.first().map_or(-1, |r| r.timestamp);
    let newest = recs.iter().map(|r| r.timestamp).max().unwrap_or(-1);
    let mut body = Vec::new();
    for (i, r) in recs.iter().enumerate() {
        let mut rec = vec![0_u8];
        varint::write(&mut rec, r.timestamp.wrapping_sub(first));
        varint::write(&mut rec, i64::try_from(i).unwrap_or(i64::MAX));
        for field in [&r.key, &r.value] {
            sized(&mut rec, field.as_deref());
        }
        varint::write(&mut rec, i64::try_from(r.headers.len()).unwrap_or(0));
        for (name, value) in &r.headers {
            sized(&mut rec, Some(name));
            sized(&mut rec, Some(value));
        }
        varint::write(&mut body, i64::try_from(rec.len()).unwrap_or(i64::MAX));
        body.extend_from_slice(&rec);
    }
    let count = i32::try_from(recs.len()).unwrap_or(i32::MAX);
    let mut b = BytesMut::with_capacity(61 + body.len());
    b.put_i64(0); // baseOffset
    b.put_i32(0); // batchLength, below
    b.put_i32(-1); // partitionLeaderEpoch
    b.put_i8(2); // magic
    b.put_u32(0); // crc, below
    b.put_u16(0); // attributes: no codec, CreateTime
    b.put_i32(count - 1); // lastOffsetDelta
    b.put_i64(first);
    b.put_i64(newest);
    b.put_i64(-1); // producerId
    b.put_i16(-1); // producerEpoch
    b.put_i32(-1); // baseSequence
    b.put_i32(count);
    b.put_slice(&body);
    let mut v = b.to_vec();
    let length = u32::try_from(v.len().saturating_sub(12)).unwrap_or(u32::MAX);
    if let Some(dst) = v.get_mut(LENGTH..LENGTH + 4) {
        dst.copy_from_slice(&length.to_be_bytes());
    }
    let crc = crc_of(&v);
    if let Some(dst) = v.get_mut(CRC..CRC + 4) {
        dst.copy_from_slice(&crc.to_be_bytes());
    }
    Bytes::from(v)
}

fn sized(out: &mut Vec<u8>, field: Option<&[u8]>) {
    match field {
        Some(bytes) => {
            varint::write(out, i64::try_from(bytes.len()).unwrap_or(i64::MAX));
            out.extend_from_slice(bytes);
        }
        None => varint::write(out, -1),
    }
}

#[cfg(test)]
mod tests;

/// A message to put in a set: its timestamp, key and value. For tests.
#[cfg(any(test, feature = "test-support"))]
pub type TestMessage<'a> = (i64, Option<&'a [u8]>, Option<&'a [u8]>);

/// A message read back: its format, offset, timestamp, key and value. For tests.
#[cfg(any(test, feature = "test-support"))]
pub type ReadMessage = (Magic, i64, i64, Option<Bytes>, Option<Bytes>);

/// A message set of format `magic` holding `messages` (timestamp, key, value), uncompressed, at offsets 0, 1, 2: what a
/// client of Produce v0 to v2 sends. For tests (feature `test-support`).
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn message_set(magic: Magic, messages: &[TestMessage<'_>]) -> Bytes {
    let mut out = Vec::new();
    for (i, (timestamp, key, value)) in messages.iter().enumerate() {
        message::write(
            &mut out,
            &message::Out {
                magic,
                offset: i64::try_from(i).unwrap_or(i64::MAX),
                timestamp: *timestamp,
                log_append_time: false,
                codec: 0,
                key: *key,
                value: *value,
            },
        );
    }
    Bytes::from(out)
}

/// The messages of a message set of format v0 or v1 (offset, timestamp, key, value), a wrapper's inner ones included, as an
/// old consumer reads them; `None` if it is not one. For tests (feature `test-support`).
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn read_set(set: &Bytes) -> Option<Vec<ReadMessage>> {
    let mut out = Vec::new();
    for entry in message::entries(set) {
        let message::Entry::Message(m) = entry.ok()? else {
            return None;
        };
        let c = codec::of(m.codec());
        if c == super::Compression::None {
            let (k, v) = m.key_value().ok()?;
            out.push((m.magic(), m.offset(), m.timestamp(), k, v));
            continue;
        }
        let (_, value) = m.key_value().ok()?;
        let inner = Bytes::from(codec::inflate(m.magic(), c, &value?, 1 << 26).ok()?);
        out.extend(read_set(&inner)?);
    }
    Some(out)
}
