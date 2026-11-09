//! Helpers for the integration tests that use the real disk.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bytes::Bytes;
use kafka_storage::fs::RealFs;
use kafka_storage::{LogConfig, PartitionLog, Trust};
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::batch::{Batch, Batches, TimestampType};
use kafka_wire::ids::{Offset, TimestampMs};

/// A directory of its own on the real disk, removed when the test ends.
pub(crate) struct Scratch(pub PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("kafka-storage-it-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn partition(&self) -> PathBuf {
        self.0.join("t-0")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) const NOW: TimestampMs = TimestampMs::from_wire(1_000_000);

pub(crate) fn open(dir: &Path, segment_bytes: u64, trust: Trust) -> PartitionLog<RealFs> {
    let config = LogConfig {
        segment_bytes,
        index_interval_bytes: 4096,
        segment_ms: None,
    };
    PartitionLog::open(RealFs, dir.to_path_buf(), config, trust).unwrap()
}

/// A one-record batch whose body is `body` bytes.
pub(crate) fn batch(body: usize) -> Batch {
    Batch::parse(
        BatchBuilder::new()
            .records(1)
            .body(vec![7; body.max(1)])
            .build(),
    )
    .unwrap()
}

pub(crate) fn append(log: &mut PartitionLog<RealFs>, batches: &[Batch]) -> Offset {
    log.append(batches, NOW, TimestampType::Create)
        .unwrap()
        .first_offset
}

/// The offsets of the batches in a read.
pub(crate) fn bases(read: &Bytes) -> Vec<i64> {
    Batches::new(read.clone())
        .map(|b| b.unwrap().base_offset().unwrap().get())
        .collect()
}
