//! The 61-byte v2 header, read once into typed fields.

use super::attributes::Attributes;
use super::error::BatchError;
use super::layout::HEADER_LEN;
use crate::ids::TimestampMs;

/// Every field of a batch header, as the sender wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Header {
    pub base_offset: i64,
    pub length: i32,
    pub leader_epoch: i32,
    pub magic: i8,
    pub crc: u32,
    pub attributes: Attributes,
    pub last_offset_delta: i32,
    pub first_timestamp: TimestampMs,
    pub max_timestamp: TimestampMs,
    pub producer_id: i64,
    pub producer_epoch: i16,
    pub base_sequence: i32,
    pub record_count: i32,
}

/// Reads big-endian fields in wire order from a slice that the caller has shown to be long enough.
struct Fields<'a>(&'a [u8]);

impl Fields<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        match self.0.split_first_chunk::<N>() {
            Some((head, tail)) => {
                self.0 = tail;
                *head
            }
            None => [0; N], // not reached: `Header::read` hands this exactly HEADER_LEN bytes
        }
    }
    fn i8(&mut self) -> i8 {
        i8::from_be_bytes(self.take())
    }
    fn i16(&mut self) -> i16 {
        i16::from_be_bytes(self.take())
    }
    fn i32(&mut self) -> i32 {
        i32::from_be_bytes(self.take())
    }
    fn i64(&mut self) -> i64 {
        i64::from_be_bytes(self.take())
    }
}

impl Header {
    /// The header at the start of `bytes`.
    pub(super) fn read(bytes: &[u8]) -> Result<Self, BatchError> {
        let head = bytes
            .get(..HEADER_LEN)
            .ok_or(BatchError::TooShort(bytes.len()))?;
        let mut f = Fields(head);
        Ok(Self {
            base_offset: f.i64(),
            length: f.i32(),
            leader_epoch: f.i32(),
            magic: f.i8(),
            crc: u32::from_be_bytes(f.take()),
            attributes: Attributes::from_bits(u16::from_be_bytes(f.take())),
            last_offset_delta: f.i32(),
            first_timestamp: TimestampMs::from_wire(f.i64()),
            max_timestamp: TimestampMs::from_wire(f.i64()),
            producer_id: f.i64(),
            producer_epoch: f.i16(),
            base_sequence: f.i32(),
            record_count: f.i32(),
        })
    }
}
