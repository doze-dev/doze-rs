//! What the console's survey of a log says (CON-500): the segments, the offsets a round removed, the deletions it kept, and
//! the planner's reading, all on the files a real round leaves, never a copy made for the test.

use std::collections::BTreeSet;

use super::compaction::{PARAMS, keyed, plain, records};
use super::*;
use crate::compact::{judge, reading};

/// The offsets the survey says are in the log: from the start to the end, but for the gaps.
fn present(walk: &Walk) -> BTreeSet<i64> {
    let gone: BTreeSet<i64> = walk
        .segments
        .iter()
        .flat_map(|s| s.gaps.iter().flat_map(|(first, n)| *first..*first + *n))
        .collect();
    (walk.start..walk.end)
        .filter(|o| !gone.contains(o))
        .collect()
}

const KEYS: [(&str, Option<&str>); 9] = [
    ("a", Some("1")),
    ("b", Some("1")),
    ("a", Some("2")),
    ("c", Some("1")),
    ("a", Some("3")),
    ("b", Some("2")),
    ("d", Some("1")),
    ("a", Some("4")),
    ("e", Some("1")),
];

#[test]
fn the_gaps_of_a_compacted_log_are_exactly_the_offsets_a_round_removed() {
    let mut log = keyed(&KEYS);
    let before = log.reader().survey(Budget::default());
    assert!(
        before.segments.iter().all(|s| s.gaps.is_empty()),
        "nothing is removed yet"
    );
    plain(&mut log).expect("due");
    let walk = log.reader().survey(Budget::default());
    let kept: BTreeSet<i64> = records(&log).iter().map(|(o, _, _)| *o).collect();
    assert_eq!(
        present(&walk),
        kept,
        "what is not in a gap is what is in the log"
    );
    assert!(!walk.partial);
    assert_eq!(walk.end, 9);
    let removed: i64 = walk.segments.iter().map(SegmentWalk::removed).sum();
    assert_eq!(removed, 9 - i64::try_from(kept.len()).unwrap());
    assert!(removed > 0, "a round took records out");
    // Every segment says how many records it holds and they add up to the log's.
    let held: u64 = walk.segments.iter().map(|s| s.records).sum();
    assert_eq!(held, kept.len() as u64);
}

#[test]
fn a_thinned_batch_places_the_offsets_it_lost_inside_it() {
    let rec = |i: i64| kafka_wire::batch::build::Rec {
        timestamp_delta: i,
        key: Some(vec![b'a' + u8::try_from(i).unwrap() % 2]),
        value: Some(b"v".to_vec()),
        ..Default::default()
    };
    let recs: Vec<_> = (0..4).map(rec).collect();
    let b = Batch::parse(kafka_wire::batch::build::BatchBuilder::from_records(5, &recs).build())
        .unwrap();
    let (_fs, _dir, mut log) = fresh(b.len() as u64);
    log.append(std::slice::from_ref(&b), NOW, TimestampType::Create)
        .unwrap();
    for k in ["x", "y", "z"] {
        let next = super::compaction::kv(k, Some("v"));
        log.append(std::slice::from_ref(&next), NOW, TimestampType::Create)
            .unwrap();
    }
    log.sync().unwrap();
    // Keys a b a b: a round keeps the later of each, so offsets 0 and 1 go from the middle of the first batch.
    plain(&mut log).expect("due");
    let walk = log.reader().survey(Budget::default());
    let kept: BTreeSet<i64> = records(&log).iter().map(|(o, _, _)| *o).collect();
    assert_eq!(present(&walk), kept);
    assert!(kept.contains(&2) && kept.contains(&3), "{kept:?}");
    let first = walk.segments.first().unwrap();
    assert_eq!(
        first.gaps,
        [(0, 2)],
        "offsets 0 and 1, from inside a batch that spans 0 to 3"
    );
    assert_eq!(first.batches, 1);
}

#[test]
fn a_deletion_a_round_kept_is_listed_with_the_time_it_goes() {
    let mut log = keyed(&[
        ("a", Some("1")),
        ("a", None),
        ("x", Some("1")),
        ("y", Some("1")),
        ("z", Some("1")),
        ("w", Some("1")),
        ("v", Some("1")),
    ]);
    assert!(log.reader().survey(Budget::default()).tombstones.is_empty());
    plain(&mut log).unwrap();
    let walk = log.reader().survey(Budget::default());
    let [t] = walk.tombstones.as_slice() else {
        panic!("one deletion is kept: {:?}", walk.tombstones);
    };
    assert_eq!((t.offset, t.key.as_slice()), (1, b"a".as_slice()));
    // The horizon is the round's time plus delete.retention.ms.
    assert_eq!(t.goes_after_ms, PARAMS.now_ms + PARAMS.delete_retention_ms);
}

#[test]
fn what_the_survey_gives_the_planner_is_what_the_cleaner_gives_it() {
    let mut log = keyed(&KEYS);
    let walk = log.reader().survey(Budget::default());
    let (facts, active_base) = walk.planner_facts();
    let end = log.end_offset().get();
    let said = judge(&facts, 0, end, active_base, &PARAMS);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], Default::default())
        .unwrap();
    assert_eq!(said.is_some(), job.is_some(), "due or not, the two agree");
    let r = reading(&facts, 0, end, active_base, &PARAMS);
    let plan = said.expect("due");
    assert_eq!(
        (r.upto, r.first_dirty, r.dirty_bytes, r.clean_bytes),
        (
            plan.upto,
            plan.first_dirty,
            plan.dirty_bytes,
            plan.clean_bytes
        )
    );
    drop(job);
    plain(&mut log).unwrap();
    // After the round the planner, given the survey again and the checkpoint the round wrote, says there is nothing to do.
    let walk = log.reader().survey(Budget::default());
    let (facts, active_base) = walk.planner_facts();
    let checkpoint = crate::compact::read_checkpoint(&log_fs(&log), &log_dir(&log)).get();
    assert_eq!(judge(&facts, checkpoint, end, active_base, &PARAMS), None);
}

fn log_fs(log: &PartitionLog<SimFs>) -> SimFs {
    log.fs.clone()
}

fn log_dir(log: &PartitionLog<SimFs>) -> std::path::PathBuf {
    log.dir.clone()
}

#[test]
fn a_budget_that_is_spent_says_the_walk_is_partial_and_never_guesses() {
    let log = keyed(&KEYS);
    let tight = Budget {
        batches: 2,
        ..Budget::default()
    };
    let walk = log.reader().survey(tight);
    assert!(walk.partial);
    assert!(walk.segments.iter().any(|s| !s.walked));
}

#[test]
fn an_empty_log_has_one_empty_active_segment_and_no_gaps() {
    let (_fs, _dir, log) = fresh(NEVER);
    let walk = log.reader().survey(Budget::default());
    assert_eq!((walk.start, walk.end), (0, 0));
    assert_eq!(walk.segments.len(), 1);
    let s = &walk.segments[0];
    assert_eq!((s.bytes, s.records, s.batches), (0, 0, 0));
    assert!(s.gaps.is_empty() && s.walked && s.newest_ms.is_none());
}
