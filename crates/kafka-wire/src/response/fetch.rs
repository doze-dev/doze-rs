//! A Fetch response written around its records, not through them.
//!
//! kafka-protocol writes `records` inline (`put_slice`), so every byte a fetch returns would be copied into the response
//! buffer before the socket saw it: 50 MiB of records, 50 MiB more memory and a 50 MiB copy. Here the envelope (every field
//! but the records) is written into one buffer, and each partition's records join the [`Frame`] as the `Bytes` the log was
//! read into, so the socket takes them from there.
//!
//! The envelope is written field by field in the order and form of kafka-protocol 0.18's generated `Encodable` for
//! `FetchResponse`, `FetchableTopicResponse` and `PartitionData` (its field writers are private to it); the structs inside
//! them (aborted transactions, and the tagged fields' structs) are written by kafka-protocol itself. The tests hold the two
//! encoders to the same bytes at every version this path takes, and the frame's length is checked against kafka-protocol's
//! own count of it.

use std::collections::BTreeMap;
use std::ops::{RangeBounds, RangeInclusive};

use bytes::{BufMut, Bytes, BytesMut};
use kafka_protocol::messages::fetch_response::{
    EpochEndOffset, FetchableTopicResponse, LeaderIdAndEpoch, PartitionData, SnapshotId,
};
use kafka_protocol::messages::{ApiKey, FetchResponse};
use kafka_protocol::protocol::{Encodable, StrBytes};

use super::{Frame, frame_len, header};
use crate::error::WireError;

/// The versions written here: from the first the broker serves (v4) to the last before v16, whose node endpoints
/// (KIP-951) this envelope does not write. Any other version goes through kafka-protocol whole.
pub(super) const VERSIONS: RangeInclusive<i16> = 4..=15;

/// Records shorter than this are copied into the envelope: under a page, the copy costs less than one more piece for the
/// socket to gather.
pub(super) const SHARED_FROM: usize = 4 << 10;

/// The first version written in the flexible form: compact strings, arrays and bytes, and tagged fields.
const FLEXIBLE: i16 = 12;

const WHAT: &str = "FetchResponse";

/// The frame for `fetch` at `version` (one of [`VERSIONS`]), with records of `shared_from` bytes or more left where they
/// are.
pub(super) fn frame(
    correlation_id: i32,
    version: i16,
    fetch: &FetchResponse,
    shared_from: usize,
) -> Result<Frame, WireError> {
    let (header, header_version) = header(correlation_id, ApiKey::Fetch, version);
    // kafka-protocol counts the frame first: that also refuses, as its encoder would, a field set at a version without it.
    let size = header
        .compute_size(header_version)
        .map_err(|e| WireError::encode("response header", e))?
        + fetch
            .compute_size(version)
            .map_err(|e| WireError::encode(WHAT, e))?;
    let shared = shared_len(fetch, shared_from);
    let mut out = Envelope {
        // Everything but the shared records: one allocation, split into pieces without a copy.
        head: BytesMut::with_capacity((4 + size).saturating_sub(shared)),
        frame: Frame::default(),
        version,
        shared_from,
    };
    out.head.put_i32(frame_len(size)?);
    header
        .encode(&mut out.head, header_version)
        .map_err(|e| WireError::encode("response header", e))?;
    out.response(fetch)?;
    let frame = out.finish();
    if bytes::Buf::remaining(&frame) != 4 + size {
        return Err(WireError::encode(
            WHAT,
            "the envelope's length is not kafka-protocol's",
        ));
    }
    Ok(frame)
}

/// How many bytes of records in `fetch` are shared rather than copied.
fn shared_len(fetch: &FetchResponse, shared_from: usize) -> usize {
    fetch
        .responses
        .iter()
        .flat_map(|t| &t.partitions)
        .filter_map(|p| p.records.as_ref().map(Bytes::len))
        .filter(|&len| len >= shared_from)
        .sum()
}

/// The frame being written: the envelope so far, and the pieces before it.
struct Envelope {
    head: BytesMut,
    frame: Frame,
    version: i16,
    shared_from: usize,
}

impl Envelope {
    fn flexible(&self) -> bool {
        self.version >= FLEXIBLE
    }

    fn response(&mut self, r: &FetchResponse) -> Result<(), WireError> {
        self.head.put_i32(r.throttle_time_ms);
        if self.version >= 7 {
            self.head.put_i16(r.error_code);
            self.head.put_i32(r.session_id);
        }
        self.array_len(Some(r.responses.len()))?;
        for topic in &r.responses {
            self.topic(topic)?;
        }
        if self.flexible() {
            // v16's node endpoints (tag 0) are past `VERSIONS`. Unknown tags are written from 1 on, as kafka-protocol
            // writes them; one below that would be counted and not written, and the length check in `frame` refuses it.
            self.varint_len(r.unknown_tagged_fields.len())?;
            self.unknown_tagged(&r.unknown_tagged_fields, 1..)?;
        }
        Ok(())
    }

    fn topic(&mut self, t: &FetchableTopicResponse) -> Result<(), WireError> {
        if self.version < 13 {
            self.string(&t.topic)?;
        } else {
            self.head.put_slice(t.topic_id.as_bytes());
        }
        self.array_len(Some(t.partitions.len()))?;
        for partition in &t.partitions {
            self.partition(partition)?;
        }
        if self.flexible() {
            self.varint_len(t.unknown_tagged_fields.len())?;
            self.unknown_tagged(&t.unknown_tagged_fields, 0..)?;
        }
        Ok(())
    }

    fn partition(&mut self, p: &PartitionData) -> Result<(), WireError> {
        self.head.put_i32(p.partition_index);
        self.head.put_i16(p.error_code);
        self.head.put_i64(p.high_watermark);
        self.head.put_i64(p.last_stable_offset);
        if self.version >= 5 {
            self.head.put_i64(p.log_start_offset);
        }
        let aborted = p.aborted_transactions.as_deref();
        self.array_len(aborted.map(<[_]>::len))?;
        for txn in aborted.unwrap_or_default() {
            self.inner(txn)?;
        }
        if self.version >= 11 {
            self.head.put_i32(*p.preferred_read_replica);
        }
        self.records(p.records.as_ref())?;
        if self.flexible() {
            // A known tagged field is written when it differs from its default, as kafka-protocol decides it.
            let diverging = Some(&p.diverging_epoch).filter(|&e| *e != EpochEndOffset::default());
            let leader = Some(&p.current_leader).filter(|&l| *l != LeaderIdAndEpoch::default());
            let snapshot = Some(&p.snapshot_id).filter(|&s| *s != SnapshotId::default());
            let known = usize::from(diverging.is_some())
                + usize::from(leader.is_some())
                + usize::from(snapshot.is_some());
            self.varint_len(p.unknown_tagged_fields.len() + known)?;
            self.tagged(0, diverging)?;
            self.tagged(1, leader)?;
            self.tagged(2, snapshot)?;
            self.unknown_tagged(&p.unknown_tagged_fields, 3..)?;
        }
        Ok(())
    }

    /// The records field: its length in the envelope, then the records themselves, shared if they are long enough.
    fn records(&mut self, records: Option<&Bytes>) -> Result<(), WireError> {
        let Some(records) = records else {
            return self.null();
        };
        if self.flexible() {
            self.varint_len(records.len() + 1)?;
        } else {
            self.head.put_i32(int32(records.len())?);
        }
        if records.len() < self.shared_from {
            self.head.put_slice(records);
        } else if !records.is_empty() {
            let envelope = self.head.split().freeze();
            self.frame.push(envelope);
            // A second reference to the buffer the log was read into, not a copy of it: the reason this module exists.
            self.frame.push(records.clone());
        }
        Ok(())
    }

    fn finish(mut self) -> Frame {
        self.frame.push(self.head.freeze());
        self.frame
    }

    /// A struct kafka-protocol writes itself, at this version.
    fn inner(&mut self, value: &impl Encodable) -> Result<(), WireError> {
        value
            .encode(&mut self.head, self.version)
            .map_err(|e| WireError::encode(WHAT, e))
    }

    /// A known tagged field, if it is set: its tag, its size, then the struct.
    fn tagged(&mut self, tag: u32, value: Option<&impl Encodable>) -> Result<(), WireError> {
        let Some(value) = value else {
            return Ok(());
        };
        let size = value
            .compute_size(self.version)
            .map_err(|e| WireError::encode(WHAT, e))?;
        self.varint(tag);
        self.varint_len(size)?;
        self.inner(value)
    }

    fn unknown_tagged(
        &mut self,
        fields: &BTreeMap<i32, Bytes>,
        tags: impl RangeBounds<i32>,
    ) -> Result<(), WireError> {
        for (&tag, value) in fields.range(tags) {
            self.varint(tag.cast_unsigned());
            self.varint_len(value.len())?;
            self.head.put_slice(value);
        }
        Ok(())
    }

    /// The count of an array (`None`: a null array).
    fn array_len(&mut self, len: Option<usize>) -> Result<(), WireError> {
        match len {
            None => self.null(),
            Some(len) if self.flexible() => self.varint_len(len + 1),
            Some(len) => {
                self.head.put_i32(int32(len)?);
                Ok(())
            }
        }
    }

    /// A null array or byte string.
    fn null(&mut self) -> Result<(), WireError> {
        if self.flexible() {
            self.varint(0);
        } else {
            self.head.put_i32(-1);
        }
        Ok(())
    }

    fn string(&mut self, s: &StrBytes) -> Result<(), WireError> {
        if self.flexible() {
            self.varint_len(s.len() + 1)?;
        } else {
            let len = i16::try_from(s.len())
                .map_err(|_| WireError::encode(WHAT, "a topic name too long"))?;
            self.head.put_i16(len);
        }
        self.head.put_slice(s.as_bytes());
        Ok(())
    }

    fn varint_len(&mut self, len: usize) -> Result<(), WireError> {
        let len =
            u32::try_from(len).map_err(|_| WireError::encode(WHAT, "a length past 32 bits"))?;
        self.varint(len);
        Ok(())
    }

    /// An unsigned varint, seven bits a byte, low bits first.
    fn varint(&mut self, mut n: u32) {
        loop {
            let [low, ..] = n.to_le_bytes();
            if n < 0x80 {
                self.head.put_u8(low);
                return;
            }
            self.head.put_u8(low | 0x80);
            n >>= 7;
        }
    }
}

fn int32(len: usize) -> Result<i32, WireError> {
    i32::try_from(len).map_err(|_| WireError::encode(WHAT, "a length past 31 bits"))
}

#[cfg(test)]
mod tests;
