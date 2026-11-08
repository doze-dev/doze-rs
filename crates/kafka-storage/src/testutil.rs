//! Helpers shared by this crate's unit tests.

use std::path::{Path, PathBuf};

use bytes::Bytes;
use kafka_wire::batch::Batch;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::ids::Offset;

use crate::fs::Fs;
use crate::sim::SimFs;

/// A simulated disk with a partition directory, both flushed.
pub(crate) fn disk() -> (SimFs, PathBuf) {
    let fs = SimFs::new();
    let dir = PathBuf::from("/data/t-0");
    fs.create_dir_all(&dir).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs.sync_dir(Path::new("/data")).unwrap();
    (fs, dir)
}

pub(crate) fn offset(n: i64) -> Offset {
    Offset::new(n).unwrap()
}

/// The raw bytes of a batch of `records` records stamped at `base`, with `max_ts` as its newest timestamp.
pub(crate) fn raw(base: i64, records: u32, max_ts: i64) -> Bytes {
    BatchBuilder::new()
        .base_offset(base)
        .records(records)
        .timestamps(max_ts, max_ts)
        .build()
}

/// A batch to append (base offset 0, as a producer writes it).
pub(crate) fn batch(records: u32, max_ts: i64) -> Batch {
    Batch::parse(raw(0, records, max_ts)).unwrap()
}

/// Back-to-back batches of the given record counts, stamped with consecutive offsets from `base`.
pub(crate) fn run(base: i64, counts: &[u32]) -> Vec<u8> {
    let mut next = base;
    let mut out = Vec::new();
    for count in counts {
        out.extend_from_slice(&raw(next, *count, 1_000 + next));
        next += i64::from(*count);
    }
    out
}
