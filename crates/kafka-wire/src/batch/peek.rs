//! A batch header read and shape-checked on its own, without the body or the checksum: enough to walk a log
//! batch by batch, to know where each ends and which offsets it covers.

use super::attributes::Attributes;
use super::error::BatchError;
use super::header::Header;
use super::layout::{HEADER_LEN, LENGTH_OVERHEAD, MAGIC_V2};
use super::{Compression, Producer, check_count};
use crate::ids::{ProducerEpoch, ProducerId, TimestampMs};

/// What the header of a batch says. The checksum has not been checked: this is for finding a batch's extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peek {
    /// The whole batch's size in bytes, header included.
    pub total_len: u64,
    /// The offset of its first record, as stamped.
    pub base_offset: i64,
    /// How many records it holds: at least one.
    pub record_count: u32,
    /// The newest timestamp in the batch.
    pub max_timestamp: TimestampMs,
    /// Codec, timestamp type, transaction bits.
    pub attributes: Attributes,
    /// The idempotent or transactional producer that wrote it, if any.
    pub producer: Option<Producer>,
}

impl Peek {
    // fuzz:parser fuzz_batch
    /// Reads the header at the start of `bytes` (which may hold more than the header, or only the header).
    ///
    /// # Errors
    /// A header that is cut short, is not format v2, declares a length too small to hold itself, or has a record
    /// count that is not positive, disagrees with `lastOffsetDelta`, or cannot fit the body it declares.
    pub fn read(bytes: &[u8]) -> Result<Self, BatchError> {
        let header = Header::read(bytes)?;
        if header.magic != MAGIC_V2 {
            return Err(BatchError::Magic(header.magic));
        }
        let declared = i64::from(header.length) + LENGTH_OVERHEAD;
        let total = u64::try_from(declared)
            .ok()
            .filter(|t| *t >= HEADER_LEN as u64)
            .ok_or(BatchError::Length {
                declared,
                present: bytes.len(),
            })?;
        let compression = header.attributes.compression();
        if let Compression::Unknown(codec) = compression {
            return Err(BatchError::UnknownCodec(codec));
        }
        let body = usize::try_from(total).unwrap_or(usize::MAX) - HEADER_LEN;
        check_count(&header, body, compression)?;
        Ok(Self {
            total_len: total,
            base_offset: header.base_offset,
            record_count: u32::try_from(header.record_count).unwrap_or(0),
            max_timestamp: header.max_timestamp,
            attributes: header.attributes,
            producer: ProducerId::new(header.producer_id).map(|id| Producer {
                id,
                epoch: ProducerEpoch::from_wire(header.producer_epoch),
                base_sequence: u32::try_from(header.base_sequence).ok(),
            }),
        })
    }
}

impl Peek {
    // fuzz:parser fuzz_batch
    /// Like [`Peek::read`], and also holds `bytes` to be exactly one whole batch with the right checksum: what
    /// [`Batch::parse`](super::Batch::parse) checks, without taking ownership of the bytes.
    ///
    /// # Errors
    /// Anything [`Peek::read`] refuses, a length that is not the bytes present, or a wrong checksum.
    pub fn read_verified(bytes: &[u8]) -> Result<Self, BatchError> {
        let peek = Self::read(bytes)?;
        if u64::try_from(bytes.len()).ok() != Some(peek.total_len) {
            return Err(BatchError::Length {
                declared: i64::try_from(peek.total_len).unwrap_or(i64::MAX),
                present: bytes.len(),
            });
        }
        let computed = super::layout::crc_of(bytes);
        let stored = Header::read(bytes)?.crc;
        if stored != computed {
            return Err(BatchError::Crc { stored, computed });
        }
        Ok(peek)
    }
}

#[cfg(test)]
mod tests;
