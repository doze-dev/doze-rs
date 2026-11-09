//! Writing a batch out again in another codec, as Kafka does when a topic's `compression.type` is not the producer's:
//! the records are read, numbered 0, 1, 2, and compressed with the topic's codec, and what follows the stream the
//! producer wrote is gone. This is the one place a batch's records are copied.

use std::io::Read;

use bytes::Bytes;

use super::inflate::{self, InflateError};
use super::layout::{ATTRIBUTES, CRC, HEADER_LEN, LENGTH, LENGTH_OVERHEAD, MAX_TIMESTAMP, crc_of};
use super::scan::RecordsRefused;
use super::{Batch, Compression, Refusal};
use crate::ids::TimestampMs;

impl Batch {
    // fuzz:parser fuzz_batch
    /// The batch with its records in `target`, numbered 0, 1, 2, each otherwise as it was, and `newest` as the newest
    /// timestamp in its header. The records have been read already (`scan`), so a fault here is the codec's.
    ///
    /// # Errors
    /// A records section that cannot be read, one that inflates past `max_inflated`, or a `target` this build has no
    /// encoder for.
    pub fn recode(
        &self,
        target: Compression,
        max_inflated: u64,
        newest: TimestampMs,
    ) -> Result<Batch, RecordsRefused> {
        let unreadable = |why: String| RecordsRefused::new(Refusal::UnknownServerError, false, why);
        let body = self.as_bytes().get(HEADER_LEN..).unwrap_or_default();
        let mut inflated = Vec::new();
        inflate::open(self.attributes().compression(), body, max_inflated)
            .and_then(|mut s| s.read_to_end(&mut inflated).map_err(|e| inflate::why(&e)))
            .map_err(|e| match e {
                InflateError::TooLarge => RecordsRefused::new(
                    Refusal::MessageTooLarge,
                    true,
                    "the records inflate past the limit",
                ),
                other => unreadable(format!(
                    "the records cannot be read to be written again: {other:?}"
                )),
            })?;
        let numbered = renumber(&inflated, self.record_count())
            .ok_or_else(|| unreadable("the records cannot be numbered again".to_owned()))?;
        let packed = pack(target, &numbered)?;

        let mut out = Vec::with_capacity(HEADER_LEN + packed.len());
        out.extend_from_slice(self.as_bytes().get(..HEADER_LEN).unwrap_or_default());
        out.extend_from_slice(&packed);
        let length = u32::try_from(out.len() - usize::try_from(LENGTH_OVERHEAD).unwrap_or(12))
            .map_err(|_| unreadable("the batch is too long to write".to_owned()))?;
        let attributes = (self.attributes().bits() & !0x07) | code(target);
        put(&mut out, LENGTH, &length.to_be_bytes());
        put(&mut out, ATTRIBUTES, &attributes.to_be_bytes());
        put(&mut out, MAX_TIMESTAMP, &newest.get().to_be_bytes());
        let crc = crc_of(&out);
        put(&mut out, CRC, &crc.to_be_bytes());
        Batch::parse(Bytes::from(out))
            .map_err(|e| unreadable(format!("the batch written again is not one: {e}")))
    }
}

pub(super) fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    if let Some(dst) = bytes.get_mut(at..at + value.len()) {
        dst.copy_from_slice(value);
    }
}

pub(super) fn code(c: Compression) -> u16 {
    match c {
        Compression::Gzip => 1,
        Compression::Snappy => 2,
        Compression::Lz4 => 3,
        Compression::Zstd => 4,
        _ => 0,
    }
}

/// A zigzag varint (Kafka's `varint` and `varlong`) off the front of `bytes`: its value and its length.
pub(super) fn read_varint(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut value = 0u64;
    for (i, shift) in (0..70).step_by(7).enumerate() {
        let byte = *bytes.get(i)?;
        value |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((((value >> 1) as i64) ^ -((value & 1) as i64), i + 1));
        }
    }
    None
}

pub(super) fn write_varint(out: &mut Vec<u8>, v: i64) {
    let mut u = ((v << 1) ^ (v >> 63)) as u64;
    while u >= 0x80 {
        out.push((u as u8 & 0x7f) | 0x80);
        u >>= 7;
    }
    out.push(u as u8);
}

/// The records of `count` records, each with its offset delta replaced by its place.
fn renumber(records: &[u8], count: u32) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(records.len());
    let mut rest = records;
    for index in 0..i64::from(count) {
        let (length, used) = read_varint(rest)?;
        let record = rest.get(used..used + usize::try_from(length).ok()?)?;
        rest = rest.get(used + usize::try_from(length).ok()?..)?;
        // attributes (one byte), timestamp delta, offset delta, then the rest as it is.
        let (attributes, after_attributes) = record.split_first()?;
        let (_, ts_len) = read_varint(after_attributes)?;
        let (ts_delta, after_ts) = after_attributes.split_at(ts_len);
        let (_, offset_len) = read_varint(after_ts)?;
        let tail = after_ts.get(offset_len..)?;
        let mut body = vec![*attributes];
        body.extend_from_slice(ts_delta);
        write_varint(&mut body, index);
        body.extend_from_slice(tail);
        write_varint(&mut out, i64::try_from(body.len()).ok()?);
        out.extend_from_slice(&body);
    }
    Some(out)
}

/// `records` compressed with `codec`.
pub(super) fn pack(codec: Compression, records: &[u8]) -> Result<Vec<u8>, RecordsRefused> {
    use std::io::Write;
    let failed = |why: String| RecordsRefused::new(Refusal::UnknownServerError, false, why);
    match codec {
        Compression::None => Ok(records.to_vec()),
        #[cfg(feature = "gzip")]
        Compression::Gzip => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(records).map_err(|e| failed(e.to_string()))?;
            e.finish().map_err(|e| failed(e.to_string()))
        }
        #[cfg(feature = "snappy")]
        Compression::Snappy => {
            // The framing snappy-java writes (and Kafka reads): a header, then blocks each led by its length.
            let mut out = vec![
                0x82, b'S', b'N', b'A', b'P', b'P', b'Y', 0, 0, 0, 0, 1, 0, 0, 0, 1,
            ];
            for chunk in records.chunks(32 << 10) {
                let block = snap::raw::Encoder::new()
                    .compress_vec(chunk)
                    .map_err(|e| failed(e.to_string()))?;
                out.extend_from_slice(
                    &u32::try_from(block.len())
                        .map_err(|e| failed(e.to_string()))?
                        .to_be_bytes(),
                );
                out.extend_from_slice(&block);
            }
            Ok(out)
        }
        #[cfg(feature = "lz4")]
        Compression::Lz4 => {
            let mut e = lz4_flex::frame::FrameEncoder::new(Vec::new());
            e.write_all(records).map_err(|e| failed(e.to_string()))?;
            e.finish().map_err(|e| failed(e.to_string()))
        }
        #[cfg(feature = "zstd")]
        Compression::Zstd => Ok(ruzstd::encoding::compress_to_vec(
            records,
            ruzstd::encoding::CompressionLevel::Fastest,
        )),
        other => Err(RecordsRefused::new(
            Refusal::UnsupportedCompression,
            true,
            format!("this build cannot write {other:?}"),
        )),
    }
}

#[cfg(test)]
mod tests;
