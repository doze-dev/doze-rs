//! Builds valid batches for tests (feature `test-support`), written from the format and not by the parser
//! they are used to test.

use bytes::{BufMut, Bytes, BytesMut};

use super::layout::{CRC, LENGTH, crc_of};

/// A record batch under construction. Defaults: one uncompressed record, producer -1, timestamps 0.
#[derive(Debug, Clone)]
pub struct BatchBuilder {
    base_offset: i64,
    count: u32,
    last_offset_delta: Option<i32>,
    attributes: u16,
    first_timestamp: i64,
    max_timestamp: i64,
    producer: (i64, i16, i32),
    body: Option<Vec<u8>>,
}

impl Default for BatchBuilder {
    fn default() -> Self {
        Self {
            base_offset: 0,
            count: 1,
            last_offset_delta: None,
            attributes: 0,
            first_timestamp: 0,
            max_timestamp: 0,
            producer: (-1, -1, -1),
            body: None,
        }
    }
}

impl BatchBuilder {
    /// A one-record batch with every default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The base offset to write.
    #[must_use]
    pub fn base_offset(mut self, v: i64) -> Self {
        self.base_offset = v;
        self
    }

    /// How many records (each a minimal null-key, empty-value record, unless a body is given).
    #[must_use]
    pub fn records(mut self, n: u32) -> Self {
        self.count = n;
        self
    }

    /// The offsets the batch spans, less one, when that is not its record count less one (a batch compaction thinned).
    #[must_use]
    pub fn last_offset_delta(mut self, v: i32) -> Self {
        self.last_offset_delta = Some(v);
        self
    }

    /// The attribute bits.
    #[must_use]
    pub fn attributes(mut self, bits: u16) -> Self {
        self.attributes = bits;
        self
    }

    /// The timestamps in the header.
    #[must_use]
    pub fn timestamps(mut self, first: i64, max: i64) -> Self {
        (self.first_timestamp, self.max_timestamp) = (first, max);
        self
    }

    /// The producer id, epoch and base sequence.
    #[must_use]
    pub fn producer(mut self, id: i64, epoch: i16, base_sequence: i32) -> Self {
        self.producer = (id, epoch, base_sequence);
        self
    }

    /// The bytes after the header, in place of generated records (for a compressed batch, say).
    #[must_use]
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// The batch, with its length and CRC filled in.
    #[must_use]
    pub fn build(self) -> Bytes {
        let body = self
            .body
            .clone()
            .unwrap_or_else(|| minimal_records(self.count));
        let mut b = BytesMut::new();
        b.put_i64(self.base_offset);
        b.put_i32(0); // batchLength, below
        b.put_i32(0); // partitionLeaderEpoch
        b.put_i8(2); // magic
        b.put_u32(0); // crc, below
        b.put_u16(self.attributes);
        b.put_i32(self.last_offset_delta.unwrap_or_else(|| {
            i32::try_from(self.count)
                .unwrap_or(i32::MAX)
                .wrapping_sub(1)
        }));
        b.put_i64(self.first_timestamp);
        b.put_i64(self.max_timestamp);
        b.put_i64(self.producer.0);
        b.put_i16(self.producer.1);
        b.put_i32(self.producer.2);
        b.put_i32(i32::try_from(self.count).unwrap_or(i32::MAX));
        b.put_slice(&body);
        let mut v = b.to_vec();
        reseal(&mut v);
        Bytes::from(v)
    }
}

/// Rewrites a batch's length and CRC to match its bytes, after a test has changed them.
pub fn reseal(batch: &mut [u8]) {
    let length = u32::try_from(batch.len().saturating_sub(12)).unwrap_or(0);
    if let Some(dst) = batch.get_mut(LENGTH..LENGTH + 4) {
        dst.copy_from_slice(&length.to_be_bytes());
    }
    let crc = crc_of(batch);
    if let Some(dst) = batch.get_mut(CRC..CRC + 4) {
        dst.copy_from_slice(&crc.to_be_bytes());
    }
}

fn zigzag(v: i64, out: &mut Vec<u8>) {
    let mut u = ((v << 1) ^ (v >> 63)) as u64;
    while u >= 0x80 {
        out.push((u as u8 & 0x7f) | 0x80);
        u >>= 7;
    }
    out.push(u as u8);
}

/// `n` records, each: attributes, timestamp delta 0, offset delta i, null key, empty value, no headers.
fn minimal_records(n: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        let mut rec = vec![0u8, 0];
        zigzag(i64::from(i), &mut rec);
        rec.extend_from_slice(&[1, 0, 0]); // key length -1, value length 0, headers 0
        zigzag(rec.len() as i64, &mut out);
        out.extend_from_slice(&rec);
    }
    out
}

/// A record to put in a batch.
#[derive(Debug, Clone, Default)]
pub struct Rec {
    /// Written this long after the batch's first timestamp.
    pub timestamp_delta: i64,
    /// Its offset within the batch, if not its place in it.
    pub offset_delta: Option<i64>,
    /// Its key, or none.
    pub key: Option<Vec<u8>>,
    /// Its value, or none.
    pub value: Option<Vec<u8>>,
    /// Its headers.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
}

/// The records section for `recs`, numbered 0, 1, 2, ...
#[must_use]
pub fn encode_records(recs: &[Rec]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, r) in recs.iter().enumerate() {
        let mut rec = vec![0u8];
        zigzag(r.timestamp_delta, &mut rec);
        zigzag(r.offset_delta.unwrap_or(i as i64), &mut rec);
        for field in [&r.key, &r.value] {
            match field {
                Some(bytes) => {
                    zigzag(bytes.len() as i64, &mut rec);
                    rec.extend_from_slice(bytes);
                }
                None => zigzag(-1, &mut rec),
            }
        }
        zigzag(r.headers.len() as i64, &mut rec);
        for (k, v) in &r.headers {
            zigzag(k.len() as i64, &mut rec);
            rec.extend_from_slice(k);
            zigzag(v.len() as i64, &mut rec);
            rec.extend_from_slice(v);
        }
        zigzag(rec.len() as i64, &mut out);
        out.extend_from_slice(&rec);
    }
    out
}

impl BatchBuilder {
    /// A batch of exactly these records, written from `first` on, with the header's newest timestamp the records'.
    #[must_use]
    pub fn from_records(first: i64, recs: &[Rec]) -> Self {
        let newest = recs
            .iter()
            .map(|r| r.timestamp_delta)
            .max()
            .map_or(first, |d| first + d);
        Self::new()
            .records(u32::try_from(recs.len()).unwrap_or(u32::MAX))
            .timestamps(first, newest)
            .body(encode_records(recs))
    }
}
