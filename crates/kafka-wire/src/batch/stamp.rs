//! What a broker may change inside a batch: its base offset, and under LogAppendTime its timestamp.
//! Nothing else in a batch is ever touched.

use bytes::{Bytes, BytesMut};

use super::Batch;
use super::layout::{ATTRIBUTES, BASE_OFFSET, CRC, MAX_TIMESTAMP, crc_of};
use crate::ids::{Offset, TimestampMs};

/// Owned, writable bytes of a batch: the same allocation when nothing else shares it, else a copy.
fn writable(bytes: Bytes) -> BytesMut {
    bytes
        .try_into_mut()
        .unwrap_or_else(|shared| BytesMut::from(&shared[..]))
}

fn put(buf: &mut BytesMut, at: usize, value: &[u8]) {
    if let Some(dst) = buf.get_mut(at..at + value.len()) {
        dst.copy_from_slice(value);
    }
}

impl Batch {
    /// This batch with the base offset the log assigned. The base offset is not covered by the CRC.
    #[must_use]
    pub fn with_base_offset(mut self, offset: Offset) -> Self {
        let mut buf = writable(self.bytes);
        put(&mut buf, BASE_OFFSET, &offset.get().to_be_bytes());
        self.header.base_offset = offset.get();
        self.bytes = buf.freeze();
        self
    }

    /// This batch marked as carrying the broker's time rather than the producer's: the timestamp type is
    /// LogAppendTime and the maximum timestamp is `now`. Both are covered by the CRC, which is recomputed.
    #[must_use]
    pub fn with_log_append_time(mut self, now: TimestampMs) -> Self {
        let attributes = self.header.attributes.with_log_append_time();
        let mut buf = writable(self.bytes);
        put(&mut buf, ATTRIBUTES, &attributes.bits().to_be_bytes());
        put(&mut buf, MAX_TIMESTAMP, &now.get().to_be_bytes());
        let crc = crc_of(&buf);
        put(&mut buf, CRC, &crc.to_be_bytes());
        self.header.attributes = attributes;
        self.header.max_timestamp = now;
        self.header.crc = crc;
        self.bytes = buf.freeze();
        self
    }
}
