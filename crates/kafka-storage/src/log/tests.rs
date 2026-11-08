//! Tests of the partition log, on the simulated disk.

mod append;
mod durability;
mod recovery;
mod trim;

use std::path::PathBuf;

use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;

use super::*;
use crate::sim::SimFs;
use crate::testutil::{batch, disk};

pub(super) fn config(segment_bytes: u64) -> LogConfig {
    LogConfig {
        segment_bytes,
        index_interval_bytes: 150,
    }
}

pub(super) fn open(
    fs: &SimFs,
    dir: &std::path::Path,
    segment_bytes: u64,
    trust: Trust,
) -> PartitionLog<SimFs> {
    PartitionLog::open(fs.clone(), dir.to_path_buf(), config(segment_bytes), trust).unwrap()
}

/// A fresh log on a fresh disk, with segments of `segment_bytes`.
pub(super) fn fresh(segment_bytes: u64) -> (SimFs, PathBuf, PartitionLog<SimFs>) {
    let (fs, dir) = disk();
    let log = open(&fs, &dir, segment_bytes, Trust::Unvouched);
    (fs, dir, log)
}

pub(super) const NEVER: u64 = u64::MAX;
pub(super) const NOW: TimestampMs = TimestampMs::from_wire(1_000_000);

/// `count` one-record batches, each with the given newest timestamp.
pub(super) fn ones(count: usize, ts: i64) -> Vec<Batch> {
    (0..count).map(|_| batch(1, ts)).collect()
}

/// The size of a one-record batch.
pub(super) fn one() -> u64 {
    batch(1, 0).len() as u64
}

/// The base offsets of the batches in a read.
pub(super) fn bases(read: &bytes::Bytes) -> Vec<i64> {
    kafka_wire::batch::Batches::new(read.clone())
        .map(|b| b.unwrap().base_offset().unwrap().get())
        .collect()
}

/// Everything a reader can read, fetching again as a consumer does: a read is served from one segment.
pub(super) fn all(reader: &LogReader<SimFs>) -> bytes::Bytes {
    let mut out = Vec::new();
    let mut from = reader.start_offset();
    loop {
        let got = reader.read(from, usize::MAX, None).unwrap();
        let Some(last) = kafka_wire::batch::Batches::new(got.clone()).last() else {
            return bytes::Bytes::from(out);
        };
        let last = last.unwrap();
        from = last
            .base_offset()
            .unwrap()
            .checked_add(last.record_count())
            .unwrap();
        out.extend_from_slice(&got);
    }
}
