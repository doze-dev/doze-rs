//! A batch of one record, written as a client writes it: what the console's Produce form sends.
//!
//! The broker checks the batch as it checks any client's (size, CRC, keys on a compacted topic, timestamps), so the
//! console meets the same refusals a producer does. This is the one writer in the crate; the builders used by tests are
//! behind the `test-support` feature and are written separately, from the format, to check this one.

use bytes::{BufMut, Bytes, BytesMut};

use super::layout::{ATTRIBUTES, CRC, LENGTH, crc_of};

/// The record to write.
#[derive(Debug, Clone, Copy)]
pub struct OneRecord<'a> {
    /// Its timestamp (CreateTime), in milliseconds since the epoch.
    pub timestamp_ms: i64,
    /// Its key, or none (a record with no key).
    pub key: Option<&'a [u8]>,
    /// Its value, or none (a record with a key and no value is a tombstone).
    pub value: Option<&'a [u8]>,
    /// Its headers, in order.
    pub headers: &'a [(&'a str, &'a [u8])],
}

/// A variable-length, zig-zag encoded integer, as the record format writes them.
fn varint(v: i64, out: &mut Vec<u8>) {
    let mut u = ((v << 1) ^ (v >> 63)) as u64;
    while u >= 0x80 {
        out.push((u as u8 & 0x7f) | 0x80);
        u >>= 7;
    }
    out.push(u as u8);
}

fn sized(field: Option<&[u8]>, out: &mut Vec<u8>) {
    match field {
        Some(bytes) => {
            varint(i64::try_from(bytes.len()).unwrap_or(i64::MAX), out);
            out.extend_from_slice(bytes);
        }
        None => varint(-1, out),
    }
}

fn record(r: &OneRecord<'_>) -> Vec<u8> {
    // Attributes (unused, 0), timestamp delta 0, offset delta 0.
    let mut body = vec![0_u8, 0, 0];
    sized(r.key, &mut body);
    sized(r.value, &mut body);
    varint(i64::try_from(r.headers.len()).unwrap_or(0), &mut body);
    for (name, value) in r.headers {
        sized(Some(name.as_bytes()), &mut body);
        sized(Some(value), &mut body);
    }
    let mut out = Vec::with_capacity(body.len() + 5);
    varint(i64::try_from(body.len()).unwrap_or(i64::MAX), &mut out);
    out.extend_from_slice(&body);
    out
}

/// One uncompressed v2 batch holding `r`, not idempotent (producer id, epoch and sequence are -1), with its length and
/// CRC filled in. The base offset is 0: the log gives the offset.
#[must_use]
pub fn single_record(r: &OneRecord<'_>) -> Bytes {
    let body = record(r);
    let mut b = BytesMut::with_capacity(61 + body.len());
    b.put_i64(0); // baseOffset
    b.put_i32(0); // batchLength, below
    b.put_i32(-1); // partitionLeaderEpoch: a client does not know it
    b.put_i8(2); // magic
    b.put_u32(0); // crc, below
    b.put_u16(0); // attributes: no compression, CreateTime, not transactional
    b.put_i32(0); // lastOffsetDelta
    b.put_i64(r.timestamp_ms);
    b.put_i64(r.timestamp_ms);
    b.put_i64(-1); // producerId
    b.put_i16(-1); // producerEpoch
    b.put_i32(-1); // baseSequence
    b.put_i32(1); // records
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
    debug_assert_eq!(ATTRIBUTES, 21);
    Bytes::from(v)
}

#[cfg(test)]
mod tests;
