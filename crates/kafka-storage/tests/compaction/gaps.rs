//! Segments removed by cleaning leave gaps between files. Everything a reader, a lookup, a trim or a restart does must be
//! the same across them as inside a cleaned segment.

use std::collections::HashMap;
use std::path::PathBuf;

use kafka_storage::sim::SimFs;
use kafka_storage::{PartitionLog, Retention, Trust};
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::batch::{Batch, Batches, TimestampType};
use kafka_wire::ids::TimestampMs;

use crate::world::{NOW, PARAMS, clean_fully, offset, open, reads_from_everywhere};

/// A batch of one record, key `key`, timestamp `ts`.
pub(crate) fn one(key: &str, ts: i64, value: Option<&str>) -> Batch {
    let rec = Rec {
        key: Some(key.as_bytes().to_vec()),
        value: value.map(|v| v.as_bytes().to_vec()),
        ..Rec::default()
    };
    Batch::parse(BatchBuilder::from_records(ts, &[rec]).build()).unwrap()
}

/// `n` batches, record i has timestamp 1000 + 10 i; every record is of key "hot" except each `every`th, which has its own.
pub(crate) fn hot(n: usize, every: usize) -> (SimFs, PathBuf, PartitionLog<SimFs>) {
    let fs = SimFs::new();
    let dir = PathBuf::from("/data/t-0");
    kafka_storage::fs::create_dirs_durably(&fs, &dir).unwrap();
    let mut log = open(&fs, &dir, Trust::Unvouched);
    for i in 0..n {
        let key = if every > 0 && i % every == every - 1 {
            format!("own{i}")
        } else {
            "hot".to_string()
        };
        let ts = 1000 + 10 * i64::try_from(i).unwrap();
        log.append(
            &[one(&key, ts, Some("vvvvvvvvvvvvvvvvvvvv"))],
            NOW,
            TimestampType::Create,
        )
        .unwrap();
    }
    log.sync().unwrap();
    (fs, dir, log)
}

pub(crate) fn all(log: &PartitionLog<SimFs>) -> Vec<(i64, i64)> {
    let reader = log.reader();
    let (mut at, end) = (log.start_offset(), log.end_offset());
    let mut out = Vec::new();
    while at < end {
        let chunk = reader.read(at, 1 << 20, None).unwrap();
        assert!(!chunk.is_empty(), "nothing at {at} before end {end}");
        for b in Batches::new(chunk) {
            let b = b.unwrap();
            let base = b.base_offset().unwrap().get();
            b.record_times()
                .map(|rs| {
                    for r in rs {
                        let r = r.unwrap();
                        out.push((base + i64::from(r.offset_delta), r.timestamp.get()));
                    }
                })
                .unwrap_or_default();
            at = b.next_offset().unwrap();
        }
    }
    out
}

pub(crate) fn files(fs: &SimFs, dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<_> = fs
        .names(dir)
        .into_iter()
        .filter(|n| n.ends_with(".log"))
        .collect();
    v.sort();
    v
}

#[test]
fn lookups_by_time_agree_with_what_a_read_finds_across_removed_segments() {
    for every in [0, 7, 13] {
        let (fs, dir, mut log) = hot(80, every);
        let before = files(&fs, &dir).len();
        clean_fully(&mut log);
        let left = all(&log);
        assert!(
            files(&fs, &dir).len() < before,
            "every {every}: no segment was removed"
        );
        reads_from_everywhere(&log, "after clean");
        let reader = log.reader();
        // ListOffsets by time: for every target the first surviving record at or after it.
        for target in 900..1900 {
            let want = left.iter().find(|(_, t)| *t >= target).copied();
            let got = reader
                .offset_for_timestamp(TimestampMs::from_wire(target))
                .unwrap()
                .map(|(o, t)| (o.get(), t.get()));
            assert_eq!(got, want, "every {every}: by time {target}");
        }
        // Max timestamp.
        let want = left.iter().max_by_key(|(o, t)| (*t, -*o)).copied();
        let got = reader
            .offset_of_max_timestamp()
            .unwrap()
            .map(|(o, t)| (o.get(), t.get()));
        assert_eq!(got, want, "every {every}: max timestamp");
        assert_eq!(log.start_offset().get(), 0);
        assert_eq!(log.end_offset().get(), 80);
        // Same after a restart.
        let again = open(&fs.process_restart(), &dir, Trust::Vouched);
        assert_eq!(all(&again), left, "every {every}: restart");
        reads_from_everywhere(&again, "restart");
    }
}

#[test]
fn delete_records_at_every_offset_after_a_clean() {
    for every in [0, 7] {
        for d in 0..=80 {
            let (fs, dir, mut log) = hot(80, every);
            clean_fully(&mut log);
            let before = all(&log);
            let start = log.delete_before(Some(offset(d))).unwrap();
            assert_eq!(start.get(), d.max(0), "start after delete {d}");
            let want: Vec<_> = before.iter().copied().filter(|(o, _)| *o >= d).collect();
            assert_eq!(all(&log), want, "every {every}, delete {d}");
            reads_from_everywhere(&log, &format!("delete {d}"));
            assert_eq!(log.end_offset().get(), 80);
            // Append, restart, read.
            log.append(&[one("hot", 5000, Some("x"))], NOW, TimestampType::Create)
                .unwrap();
            log.sync().unwrap();
            drop(log);
            let again = open(&fs.process_restart(), &dir, Trust::Vouched);
            assert_eq!(
                again.start_offset().get(),
                d,
                "start after restart, delete {d}"
            );
            assert_eq!(again.end_offset().get(), 81);
            let mut w = want.clone();
            w.push((80, 5000));
            assert_eq!(all(&again), w, "every {every}, delete {d} then restart");
            // cleaning again does not mind
            let mut again = again;
            clean_fully(&mut again);
            reads_from_everywhere(&again, &format!("delete {d}, cleaned again"));
            assert_eq!(again.start_offset().get(), d);
            assert_eq!(again.end_offset().get(), 81);
        }
    }
}

#[test]
fn retention_after_a_clean_with_gaps() {
    for every in [0, 7] {
        for max_bytes in [1u64, 100, 400, 1000, 100_000] {
            for max_age in [None, Some(1i64), Some(1_000_000_000)] {
                let (fs, dir, mut log) = hot(80, every);
                clean_fully(&mut log);
                let before = all(&log);
                let start0 = log.start_offset().get();
                let policy = Retention {
                    max_bytes: Some(max_bytes),
                    max_age_ms: max_age,
                };
                log.enforce_retention(policy, NOW).unwrap();
                let start = log.start_offset().get();
                assert!(start >= start0);
                let what = format!("every {every}, bytes {max_bytes}, age {max_age:?}");
                let want: Vec<_> = before
                    .iter()
                    .copied()
                    .filter(|(o, _)| *o >= start)
                    .collect();
                assert_eq!(all(&log), want, "{what}");
                reads_from_everywhere(&log, &what);
                assert_eq!(log.end_offset().get(), 80, "{what}");
                drop(log);
                let again = open(&fs.process_restart(), &dir, Trust::Vouched);
                assert_eq!(
                    again.start_offset().get(),
                    start,
                    "{what}: start after restart"
                );
                assert_eq!(all(&again), want, "{what}: restart");
            }
        }
    }
}

/// Run the round, with the writer as the broker has it, and a segment removal for each of many log shapes, then verify the
/// producer sequence state of a producer whose last batch sits in a segment that is otherwise emptied.
#[test]
fn a_producers_last_batch_survives_with_its_segment() {
    let fs = SimFs::new();
    let dir = PathBuf::from("/data/t-0");
    kafka_storage::fs::create_dirs_durably(&fs, &dir).unwrap();
    let mut log = open(&fs, &dir, Trust::Unvouched);
    for i in 0..40 {
        let rec = Rec {
            key: Some(b"hot".to_vec()),
            value: Some(b"vvvvvvvvvvvvvvvvvvvv".to_vec()),
            ..Rec::default()
        };
        let mut b = BatchBuilder::from_records(1000 + i, &[rec]);
        if i == 10 {
            b = b.producer(77, 3, 41);
        }
        log.append(
            &[Batch::parse(b.build()).unwrap()],
            NOW,
            TimestampType::Create,
        )
        .unwrap();
    }
    log.sync().unwrap();
    let lasts = HashMap::from([(
        77,
        kafka_storage::compact::LastOf {
            data_last_offset: Some(10),
            marker_epoch: 3,
        },
    )]);
    for _ in 0..5 {
        let Some(job) = log
            .clean_job(&PARAMS, log.end_offset(), vec![], lasts.clone())
            .unwrap()
        else {
            break;
        };
        let c = job.run().unwrap();
        log.install_cleaned(&c).unwrap();
    }
    let again = open(&fs.process_restart(), &dir, Trust::Vouched);
    let history = again.reader().producer_history(1 << 20).unwrap();
    let mine: Vec<_> = history.iter().filter(|p| p.producer.is_some()).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].base_offset, 10);
}
