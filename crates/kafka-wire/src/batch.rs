//! Record batches, format v2: the unit Kafka stores, ships and acknowledges.
//!
//! A batch is a 61-byte header and the records. The log keeps batches verbatim and works from the header
//! alone, so this module reads and checks the header and never decodes a record. Everything a broker may
//! change inside a batch (its base offset, and under LogAppendTime its timestamp) is in the private `stamp` module.

mod attributes;
#[cfg(any(test, feature = "test-support"))]
pub mod build;
mod compose;
mod error;
mod filter;
mod header;
mod inflate;
mod iter;
mod layout;
mod look;
mod marker;
mod peek;
mod produce;
mod recode;
mod records;
mod scan;
mod stamp;

use bytes::Bytes;

pub use attributes::{Attributes, Compression, TimestampType};
pub use compose::{OneRecord, single_record};
pub use error::{BatchError, Refusal};
pub use filter::{Filtered, Rules, Seen};
use header::Header;
pub use iter::Batches;
pub use layout::HEADER_LEN;
use layout::{LENGTH_OVERHEAD, MAGIC_V2};
pub use look::{Looked, LookedHeader};
pub use marker::Marker;
pub use peek::Peek;
pub use produce::{ProduceError, ProduceRules};
pub use records::{RecordTime, RecordTimes};
pub use scan::{RecordError, RecordsRefused, ScanRules, Scanned};
pub use stamp::Stamped;

use crate::ids::{Offset, ProducerEpoch, ProducerId, TimestampMs};

/// The most records one compressed batch may claim. A compressed batch can be dense, but not without limit.
const MAX_COMPRESSED_RECORDS: i32 = 16 << 20;

/// The idempotent or transactional producer that wrote a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Producer {
    /// Its id.
    pub id: ProducerId,
    /// Its epoch.
    pub epoch: ProducerEpoch,
    /// The sequence number of the batch's first record; `None` for a transaction marker, which has none.
    pub base_sequence: Option<u32>,
}

/// One whole, valid record batch: v2, its length and record count consistent, its CRC right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    bytes: Bytes,
    header: Header,
}

impl Batch {
    // fuzz:parser fuzz_batch
    /// Checks `bytes` is exactly one valid batch. The checks run in the order Kafka runs them (shape,
    /// checksum, codec, count), because that decides which fault a request is told about.
    ///
    /// # Errors
    /// Any of the [`BatchError`] shape and checksum failures.
    pub fn parse(bytes: Bytes) -> Result<Self, BatchError> {
        Self::parse_as(bytes, false)
    }

    // fuzz:parser fuzz_batch
    /// Like [`Batch::parse`], for a batch the log holds: one that compaction has thinned has fewer records than its
    /// offsets span (possibly none), which a batch sent by a client may not.
    ///
    /// # Errors
    /// As [`Batch::parse`], except for the count: it may be anything from none to the span of the offsets.
    pub fn parse_stored(bytes: Bytes) -> Result<Self, BatchError> {
        Self::parse_as(bytes, true)
    }

    fn parse_as(bytes: Bytes, stored: bool) -> Result<Self, BatchError> {
        let header = Header::read(&bytes)?;
        if header.magic != MAGIC_V2 {
            return Err(BatchError::Magic(header.magic));
        }
        let declared = i64::from(header.length) + LENGTH_OVERHEAD;
        if usize::try_from(declared).ok() != Some(bytes.len()) {
            return Err(BatchError::Length {
                declared,
                present: bytes.len(),
            });
        }
        let computed = layout::crc_of(&bytes);
        if header.crc != computed {
            return Err(BatchError::Crc {
                stored: header.crc,
                computed,
            });
        }
        let compression = header.attributes.compression();
        if let Compression::Unknown(codec) = compression {
            return Err(BatchError::UnknownCodec(codec));
        }
        check_count(&header, bytes.len() - HEADER_LEN, compression, stored)?;
        Ok(Self { bytes, header })
    }

    /// The batch as bytes, for the log or the wire.
    #[must_use]
    pub fn as_bytes(&self) -> &Bytes {
        &self.bytes
    }

    /// The batch as bytes, consumed.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        self.bytes
    }

    /// Its size in bytes.
    #[must_use]
    #[allow(clippy::len_without_is_empty)] // a batch is never empty
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// The base offset the sender wrote (a producer writes 0; the broker assigns the real one on append),
    /// or `None` if it is negative.
    #[must_use]
    pub fn base_offset(&self) -> Option<Offset> {
        Offset::new(self.header.base_offset)
    }

    /// How many records the batch holds: at least one for a batch a client sent; a stored one may hold fewer than its offsets span.
    #[must_use]
    pub fn record_count(&self) -> u32 {
        u32::try_from(self.header.record_count).unwrap_or(0)
    }

    /// The offset after the batch's last, from its base offset and the span its header declares. For a batch the log
    /// holds this is not base plus record count: compaction leaves a batch its span when it takes records out.
    #[must_use]
    pub fn next_offset(&self) -> Option<Offset> {
        let span = u32::try_from(self.header.last_offset_delta)
            .ok()?
            .checked_add(1)?;
        self.base_offset()?.checked_add(span)
    }

    /// The attributes: codec, timestamp type, transactional and control bits.
    #[must_use]
    pub fn attributes(&self) -> Attributes {
        self.header.attributes
    }

    /// The timestamp the records' deltas are relative to.
    #[must_use]
    pub fn first_timestamp(&self) -> TimestampMs {
        self.header.first_timestamp
    }

    /// The newest timestamp in the batch.
    #[must_use]
    pub fn max_timestamp(&self) -> TimestampMs {
        self.header.max_timestamp
    }

    /// The producer, if the batch came from an idempotent or transactional one.
    #[must_use]
    pub fn producer(&self) -> Option<Producer> {
        Some(Producer {
            id: ProducerId::new(self.header.producer_id)?,
            epoch: ProducerEpoch::from_wire(self.header.producer_epoch),
            base_sequence: u32::try_from(self.header.base_sequence).ok(),
        })
    }

    /// The leader epoch the batch was written under, if the writer knew it.
    #[must_use]
    pub fn partition_leader_epoch(&self) -> Option<u32> {
        u32::try_from(self.header.leader_epoch).ok()
    }
}

/// The record count must be positive, agree with `lastOffsetDelta`, and be a number the bytes could hold.
fn check_count(
    header: &Header,
    body: usize,
    compression: Compression,
    stored: bool,
) -> Result<(), BatchError> {
    let (count, delta) = (header.record_count, header.last_offset_delta);
    let agrees = if stored {
        count >= 0 && delta >= 0 && delta.checked_add(1).is_some_and(|span| count <= span)
    } else {
        count > 0 && delta.checked_add(1) == Some(count)
    };
    if !agrees {
        return Err(BatchError::Count {
            count,
            last_offset_delta: delta,
        });
    }
    if compression == Compression::None {
        if usize::try_from(count).is_ok_and(|c| c > body) {
            return Err(BatchError::CountExceedsBody { count, body });
        }
    } else if count > MAX_COMPRESSED_RECORDS {
        return Err(BatchError::TooManyRecords(count));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
