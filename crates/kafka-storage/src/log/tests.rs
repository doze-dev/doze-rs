//! Tests of the partition log, on the simulated disk.

mod append;
mod compaction;
mod compaction_txn;
mod durability;
mod history;
mod open;
mod read;
mod recovery;
mod retention;
mod tickets;
mod times;
mod transactional;
mod trim;

use std::path::PathBuf;

use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;

use super::*;
use crate::sim::{Op, SimFs};
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

fn names(fs: &SimFs, dir: &std::path::Path) -> Vec<String> {
    fs.names(dir)
        .into_iter()
        .filter(|n| n.ends_with(".log"))
        .collect()
}

/// Six one-record batches in segments of two: 0, 2, 4 (the last active).
fn three_segments(ts: i64) -> (SimFs, std::path::PathBuf, PartitionLog<SimFs>) {
    let (fs, dir, mut log) = fresh(2 * one());
    for _ in 0..6 {
        log.append(&ones(1, ts), NOW, TimestampType::Create)
            .unwrap();
    }
    (fs, dir, log)
}

/// A log with `n` one-record batches, flushed, then "killed": the disk the next process starts from.
fn written(n: usize, segment_bytes: u64) -> (SimFs, std::path::PathBuf) {
    let (fs, dir, mut log) = fresh(segment_bytes);
    for _ in 0..n {
        log.append(&ones(1, 5), NOW, TimestampType::Create).unwrap();
    }
    log.sync().unwrap();
    drop(log);
    (fs.process_restart(), dir)
}

fn reopen(fs: &SimFs, dir: &std::path::Path) -> PartitionLog<SimFs> {
    open(fs, dir, NEVER, Trust::Unvouched)
}

/// Runs a future to its end, or fails the test after five seconds: a ticket that waits for ever is a bug, and a
/// test that waits for ever with it is no way to find out.
fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), f)
                .await
                .expect("waited five seconds: the future never finishes")
        })
}

pub(super) const LOG0: &str = "00000000000000000000.log";
pub(super) const INDEX0: &str = "00000000000000000000.index";

pub(super) fn fsyncs(fs: &SimFs) -> usize {
    fs.trace()
        .iter()
        .filter(|(op, _)| matches!(op, Op::Fsync | Op::DirSync))
        .count()
}
