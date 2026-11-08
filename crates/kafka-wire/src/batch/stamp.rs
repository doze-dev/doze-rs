//! What a broker changes in a batch as it takes it: the base offset, and under LogAppendTime the timestamp type,
//! the maximum timestamp and (because they are covered by it) the checksum. Nothing else is ever touched.
//!
//! Nothing is changed in place and the batch is not copied. The changes are made to a copy of the 61-byte
//! header, on the stack, and the body is borrowed from the batch as it arrived; a writer puts the two down one
//! after the other. For LogAppendTime the new checksum is the old header region's CRC extended over the body,
//! so the body is read once and not written to memory a second time.

use super::Batch;
use super::layout::{ATTRIBUTES, BASE_OFFSET, CRC, HEADER_LEN, MAX_TIMESTAMP};
use crate::ids::{Offset, TimestampMs};

/// A batch as the log will hold it: the header with the broker's changes, and the body as it arrived.
#[derive(Debug, Clone, Copy)]
pub struct Stamped<'a> {
    header: [u8; HEADER_LEN],
    body: &'a [u8],
    base: Offset,
    max_timestamp: TimestampMs,
    record_count: u32,
}

impl Batch {
    /// The batch with its newest timestamp said to be `max`: what a broker sets from the records it has read, since a
    /// producer may write anything in the header. The bytes are not touched; the header copy that [`Batch::stamped`]
    /// makes carries the change, and the checksum with it.
    #[must_use]
    pub fn with_max_timestamp(mut self, max: TimestampMs) -> Self {
        self.header.max_timestamp = max;
        self
    }

    /// The batch stamped with `base`; with `log_append_time`, also marked as carrying the broker's clock, with
    /// that time as its newest timestamp.
    #[must_use]
    pub fn stamped(&self, base: Offset, log_append_time: Option<TimestampMs>) -> Stamped<'_> {
        let bytes = self.as_bytes();
        let mut header = bytes
            .first_chunk::<HEADER_LEN>()
            .copied()
            .unwrap_or([0; HEADER_LEN]);
        let body = bytes.get(HEADER_LEN..).unwrap_or_default();
        put(&mut header, BASE_OFFSET, &base.get().to_be_bytes());
        let mut max_timestamp = self.max_timestamp();
        let as_sent = header
            .get(MAX_TIMESTAMP..MAX_TIMESTAMP + 8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map(i64::from_be_bytes);
        let mut changed = as_sent != Some(max_timestamp.get());
        if let Some(now) = log_append_time {
            let attributes = self.attributes().with_log_append_time();
            put(&mut header, ATTRIBUTES, &attributes.bits().to_be_bytes());
            max_timestamp = now;
            changed = true;
        }
        if changed {
            put(
                &mut header,
                MAX_TIMESTAMP,
                &max_timestamp.get().to_be_bytes(),
            );
            let covered = header.get(ATTRIBUTES..).unwrap_or_default();
            let crc = crc32c::crc32c_append(crc32c::crc32c(covered), body);
            put(&mut header, CRC, &crc.to_be_bytes());
        }
        Stamped {
            header,
            body,
            base,
            max_timestamp,
            record_count: self.record_count(),
        }
    }
}

fn put(header: &mut [u8; HEADER_LEN], at: usize, value: &[u8]) {
    if let Some(dst) = header.get_mut(at..at + value.len()) {
        dst.copy_from_slice(value);
    }
}

impl Stamped<'_> {
    /// The header with the changes applied.
    #[must_use]
    pub fn header(&self) -> &[u8] {
        &self.header
    }

    /// The records, as they arrived.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        self.body
    }

    /// The whole batch's size in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        HEADER_LEN + self.body.len()
    }

    /// Whether the batch is empty: never, a batch has a header.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The offset of its first record.
    #[must_use]
    pub fn base_offset(&self) -> Offset {
        self.base
    }

    /// The newest timestamp in the batch, after stamping.
    #[must_use]
    pub fn max_timestamp(&self) -> TimestampMs {
        self.max_timestamp
    }

    /// How many records it holds.
    #[must_use]
    pub fn record_count(&self) -> u32 {
        self.record_count
    }

    /// The batch as one buffer. This copies; it exists for tests and fuzzing, which want to look at the whole.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn to_bytes(&self) -> bytes::Bytes {
        let mut out = Vec::with_capacity(self.len());
        out.extend_from_slice(&self.header);
        out.extend_from_slice(self.body);
        bytes::Bytes::from(out)
    }
}
