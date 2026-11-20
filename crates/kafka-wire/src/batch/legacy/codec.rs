//! A wrapper message's value, in and out of its codec. The codecs are the record batch's (gzip, snappy in snappy-java's
//! framing, LZ4 frames), with one quirk of format v0: Kafka wrote its LZ4 frames with the header checksum taken over the
//! frame's magic number as well as its descriptor (KAFKA-3160), and reads them back without looking at that byte. A frame
//! of format v0 is therefore read with its checksum put right first, and written with Kafka's.

use std::io::Read;

use super::Magic;
use super::checksum::xxh32;
use crate::batch::inflate::{self, InflateError};
use crate::batch::{Compression, recode};

/// The codec a number names in a message's attributes, if Kafka knows it.
pub(super) fn of(number: u8) -> Compression {
    match number {
        0 => Compression::None,
        1 => Compression::Gzip,
        2 => Compression::Snappy,
        3 => Compression::Lz4,
        4 => Compression::Zstd,
        n => Compression::Unknown(n),
    }
}

/// The number of a codec, as attributes carry it.
pub(super) fn number(c: Compression) -> u8 {
    u8::try_from(recode::code(c)).unwrap_or(0)
}

/// Where an LZ4 frame's header checksum is, if `frame` begins with a frame header: after the magic number, the flags,
/// the block descriptor and, if the flags say so, the content size and the dictionary id.
fn header_checksum_at(frame: &[u8]) -> Option<usize> {
    if frame.get(..4)? != [0x04, 0x22, 0x4d, 0x18] {
        return None;
    }
    let flags = *frame.get(4)?;
    let mut at = 6;
    if flags & 0x08 != 0 {
        at += 8;
    }
    if flags & 0x01 != 0 {
        at += 4;
    }
    (frame.len() > at).then_some(at)
}

/// The header checksum an LZ4 frame should have, and the one Kafka wrote in format v0.
fn checksums(frame: &[u8], at: usize) -> (u8, u8) {
    let right = (xxh32(frame.get(4..at).unwrap_or_default()) >> 8) as u8;
    let kafka_v0 = (xxh32(frame.get(..at).unwrap_or_default()) >> 8) as u8;
    (right, kafka_v0)
}

/// The inner message set of a wrapper of `magic` whose value is `value`, compressed with `codec`, inflated to at most
/// `limit` bytes.
pub(super) fn inflate(
    magic: Magic,
    codec: Compression,
    value: &[u8],
    limit: u64,
) -> Result<Vec<u8>, InflateError> {
    let fixed;
    let value = if magic == Magic::V0 && codec == Compression::Lz4 {
        let mut frame = value.to_vec();
        if let Some(at) = header_checksum_at(&frame) {
            let (right, _) = checksums(&frame, at);
            if let Some(b) = frame.get_mut(at) {
                *b = right;
            }
        }
        fixed = frame;
        fixed.as_slice()
    } else {
        value
    };
    let mut out = Vec::new();
    inflate::open(codec, value, limit)?
        .read_to_end(&mut out)
        .map_err(|e| inflate::why(&e))?;
    Ok(out)
}

/// `set` compressed with `codec` for a wrapper of `magic`: as a record batch's records are, and an LZ4 frame of format
/// v0 with the header checksum Kafka writes there.
pub(super) fn deflate(magic: Magic, codec: Compression, set: &[u8]) -> Option<Vec<u8>> {
    let mut packed = recode::pack(codec, set).ok()?;
    if magic == Magic::V0
        && codec == Compression::Lz4
        && let Some(at) = header_checksum_at(&packed)
    {
        let (_, kafka_v0) = checksums(&packed, at);
        if let Some(b) = packed.get_mut(at) {
            *b = kafka_v0;
        }
    }
    Some(packed)
}
