//! Rounds of cleaning on the real disk: files are really replaced and removed while readers on other threads read.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kafka_storage::compact::Params;
use kafka_storage::fs::RealFs;
use kafka_storage::{LogReader, PartitionLog, StorageError, Trust};
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::batch::{Batch, Batches};

use crate::offset;
use crate::support::{Scratch, append, open};

const PARAMS: Params = Params {
    min_dirty_ratio: 0.0,
    min_lag_ms: 0,
    max_lag_ms: i64::MAX,
    delete_retention_ms: 1_000,
    now_ms: 10,
    map_bytes: 1 << 20,
};

/// A one-record batch for `key`.
fn keyed(key: &str) -> Batch {
    let rec = Rec {
        key: Some(key.as_bytes().to_vec()),
        value: Some(vec![7; 100]),
        ..Rec::default()
    };
    Batch::parse(BatchBuilder::from_records(5, &[rec]).build()).unwrap()
}

/// Every record in the log: (offset, key).
fn records<F: kafka_storage::fs::Fs>(log: &PartitionLog<F>) -> Vec<(i64, String)> {
    let reader = log.reader();
    let (mut at, end) = (log.start_offset(), log.end_offset());
    let mut out = Vec::new();
    while at < end {
        let chunk = reader.read(at, 1 << 20, None).unwrap();
        assert!(!chunk.is_empty(), "nothing to read at {at}");
        for b in Batches::new(chunk) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                let key = String::from_utf8(s.key.unwrap_or_default().to_vec()).unwrap();
                out.push((s.offset, key));
            })
            .unwrap();
            at = b.next_offset().unwrap();
        }
    }
    out
}

/// Reads from random places in the log until told to stop: every read inside the log answers with whole batches, the
/// first of which ends past where the read began. Says how many reads it made.
fn read_until_done(reader: &LogReader<RealFs>, done: &AtomicBool, t: u64) -> u64 {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(t + 1) | 1;
    let mut reads = 0u64;
    while !done.load(Ordering::Acquire) {
        let (s, e) = (reader.start_offset().get(), reader.end_offset().get());
        if e <= s {
            continue;
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let from = s + i64::try_from(state % u64::try_from(e - s).unwrap()).unwrap();
        match reader.read(offset(from), 8 << 10, None) {
            Ok(bytes) => {
                let mut seen = Batches::new(bytes);
                let first = seen.next().expect("a read inside the log found nothing");
                let first = first.expect("a reader was handed a damaged batch");
                assert!(first.next_offset().unwrap().get() > from);
                for b in seen {
                    b.expect("a reader was handed a damaged batch");
                }
            }
            Err(StorageError::OutOfRange { .. }) => {}
            Err(e) => panic!("reading from {from}: {e}"),
        }
        reads += 1;
    }
    reads
}

#[test]
fn readers_read_whole_batches_while_rounds_clean_and_remove_segments() {
    let scratch = Scratch::new();
    let mut log = open(&scratch.partition(), 2 << 10, Trust::Unvouched);
    let done = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..3u64)
        .map(|t| {
            let (reader, done) = (log.reader(), Arc::clone(&done));
            std::thread::spawn(move || read_until_done(&reader, &done, t))
        })
        .collect();
    let mut rounds = 0;
    for round in 0..12 {
        for i in 0..40 {
            // A hot key, and now and then one of its own.
            let key = if i % 20 == 19 {
                format!("own{round}-{i}")
            } else {
                "hot".to_string()
            };
            append(&mut log, &[keyed(&key)]);
        }
        log.sync().unwrap();
        let Some(job) = log
            .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
            .unwrap()
        else {
            continue;
        };
        let cleaned = job.run().unwrap();
        log.install_cleaned(&cleaned).unwrap();
        rounds += 1;
    }
    done.store(true, Ordering::Release);
    let reads: u64 = readers.into_iter().map(|r| r.join().unwrap()).sum();
    assert!(rounds >= 6, "only {rounds} rounds were due");
    assert!(reads > 0, "the readers read nothing");

    // What is left: the latest of the hot key where it was written, and every key of its own; not a file for each segment
    // the hot key filled (some 44 were written).
    let left = records(&log);
    let last_hot = 12 * 40 - 2;
    assert!(left.contains(&(last_hot, "hot".to_string())), "{left:?}");
    assert_eq!(
        left.iter().filter(|r| r.1.starts_with("own")).count(),
        12 * 2
    );
    let names: Vec<_> = std::fs::read_dir(scratch.partition())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.contains(".tmp")), "{names:?}");
    assert!(
        names.iter().filter(|n| n.ends_with(".log")).count() < 32,
        "segments pile up: {names:?}"
    );
    let end = log.end_offset();
    log.close().unwrap();
    let again = open(&scratch.partition(), 2 << 10, Trust::Unvouched);
    assert_eq!(again.end_offset(), end);
    assert_eq!(records(&again), left);
}
