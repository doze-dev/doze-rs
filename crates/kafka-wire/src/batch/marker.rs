//! Transaction markers: the control batch a coordinator appends to each partition a transaction wrote to, to say the
//! transaction was committed or aborted.
//!
//! A marker is a batch of one record, with the control and transactional attribute bits, the producer's id and epoch and
//! no sequence. Its key is a version (`i16`, 0) and a type (`i16`: 0 abort, 1 commit); its value is a version (`i16`, 0)
//! and the coordinator's epoch (`i32`).

use bytes::{BufMut, Bytes, BytesMut};

use super::layout::{ATTRIBUTES, CRC, HEADER_LEN, LENGTH, crc_of};
use super::varint;
use crate::ids::{ProducerEpoch, ProducerId, TimestampMs};

/// The attribute bits of a transactional control batch.
const CONTROL_TRANSACTIONAL: u16 = 0x20 | 0x10;

/// What a marker says of its transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// The transaction's records are to be read.
    Commit,
    /// The transaction's records are to be skipped.
    Abort,
}

impl Marker {
    fn code(self) -> i16 {
        match self {
            Self::Abort => 0,
            Self::Commit => 1,
        }
    }

    /// The marker batch for the producer's transaction, ready to be appended: one record, no sequence.
    #[must_use]
    pub fn batch(
        self,
        producer: ProducerId,
        epoch: ProducerEpoch,
        coordinator_epoch: i32,
        at: TimestampMs,
    ) -> Bytes {
        let mut record = vec![0_u8]; // attributes
        varint::write(&mut record, 0); // timestamp delta
        varint::write(&mut record, 0); // offset delta
        varint::write(&mut record, 4); // key length
        record.put_i16(0);
        record.put_i16(self.code());
        varint::write(&mut record, 6); // value length
        record.put_i16(0);
        record.put_i32(coordinator_epoch);
        varint::write(&mut record, 0); // no headers
        let mut body = Vec::new();
        varint::write(&mut body, i64::try_from(record.len()).unwrap_or(0));
        body.extend_from_slice(&record);

        let mut b = BytesMut::new();
        b.put_i64(0); // base offset, stamped by the log
        b.put_i32(0); // batch length, below
        b.put_i32(-1); // partition leader epoch
        b.put_i8(2); // magic
        b.put_u32(0); // crc, below
        b.put_u16(CONTROL_TRANSACTIONAL);
        b.put_i32(0); // last offset delta
        b.put_i64(at.get());
        b.put_i64(at.get());
        b.put_i64(producer.get());
        b.put_i16(epoch.get());
        b.put_i32(-1); // no sequence
        b.put_i32(1); // one record
        b.put_slice(&body);
        let mut v = b.to_vec();
        let length = u32::try_from(v.len().saturating_sub(12)).unwrap_or(0);
        if let Some(dst) = v.get_mut(LENGTH..LENGTH + 4) {
            dst.copy_from_slice(&length.to_be_bytes());
        }
        let crc = crc_of(&v);
        if let Some(dst) = v.get_mut(CRC..CRC + 4) {
            dst.copy_from_slice(&crc.to_be_bytes());
        }
        debug_assert!(v.len() > HEADER_LEN && ATTRIBUTES < HEADER_LEN);
        Bytes::from(v)
    }

    /// What the control batch `bytes` (one whole batch) says, if it is a commit or an abort marker.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let mut rest = bytes.get(HEADER_LEN..)?;
        let _length = varint::read(&mut rest)?;
        let (_attributes, mut rest) = rest.split_first()?;
        let _timestamp_delta = varint::read(&mut rest)?;
        let _offset_delta = varint::read(&mut rest)?;
        let key_len = varint::read(&mut rest)?;
        let key = rest.get(..usize::try_from(key_len).ok().filter(|n| *n >= 4)?)?;
        match i16::from_be_bytes(*key.get(2..4)?.first_chunk()?) {
            0 => Some(Self::Abort),
            1 => Some(Self::Commit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::Batch;

    fn marker(kind: Marker) -> Bytes {
        kind.batch(
            ProducerId::new(7).unwrap(),
            ProducerEpoch::from_wire(3),
            5,
            TimestampMs::from_wire(1_000),
        )
    }

    #[test]
    fn a_marker_is_a_valid_control_batch_of_the_producer_with_no_sequence() {
        for kind in [Marker::Commit, Marker::Abort] {
            let bytes = marker(kind);
            let batch = Batch::parse(bytes.clone()).expect("a valid batch");
            assert!(batch.attributes().is_control() && batch.attributes().is_transactional());
            assert_eq!(batch.record_count(), 1);
            let producer = batch.producer().unwrap();
            assert_eq!(
                (
                    producer.id.get(),
                    producer.epoch.get(),
                    producer.base_sequence
                ),
                (7, 3, None)
            );
            assert_eq!(Marker::read(&bytes), Some(kind));
        }
    }
}
