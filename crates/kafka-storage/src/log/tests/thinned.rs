//! A batch cleaning has thinned spans the offsets it had, whatever records are left in it (CMP-6): the offset after it
//! is its base plus its last offset delta plus one, not its base plus the records it holds. Each test lays down a log
//! with such a batch where a part of the log works out what comes after a batch, on the files as cleaning leaves them.

use std::path::Path;

use bytes::Bytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::batch::{Batch, Rules};

use super::*;
use crate::fs::{Fs, FsFile};
use crate::testutil::{offset, raw};

/// A batch of three records (keys a, b, c) at `base`, `base + 1`, `base + 2` stamped `ts`, `ts + 1` and `ts + 2`, from
/// producer 7, of which cleaning has left only the one at `base + 1`: one record in a batch spanning three offsets.
fn thinned(base: i64, ts: i64) -> Bytes {
    let recs: Vec<_> = (0..3)
        .map(|i| Rec {
            timestamp_delta: i,
            key: Some(vec![b'a' + u8::try_from(i).unwrap()]),
            value: Some(b"v".to_vec()),
            ..Rec::default()
        })
        .collect();
    let built = BatchBuilder::from_records(ts, &recs)
        .base_offset(base)
        .producer(7, 0, 0)
        .build();
    let rules = Rules {
        max_inflated: 1 << 20,
        horizon: 0,
        marker_of_empty_transaction: false,
    };
    let filtered = Batch::parse_stored(built)
        .unwrap()
        .filter(rules, |seen| seen.offset == base + 1)
        .unwrap();
    assert_eq!(filtered.kept, 1, "one record is left of three");
    filtered.batch.into_bytes()
}

fn put(fs: &SimFs, dir: &Path, name: &str, bytes: &[u8]) {
    let (file, _) = fs.open_or_create(&dir.join(name)).unwrap();
    file.write_all_at(bytes, 0).unwrap();
}

/// One segment: a record at 0, then the thinned batch spanning 1, 2 and 3. The log ends at 4.
fn ends_thinned() -> (SimFs, std::path::PathBuf) {
    let (fs, dir) = crate::testutil::disk();
    let mut bytes = raw(0, 1, 5).to_vec();
    bytes.extend_from_slice(&thinned(1, 100));
    put(&fs, &dir, LOG0, &bytes);
    (fs, dir)
}

#[test]
fn a_log_that_ends_in_a_thinned_batch_ends_after_the_offsets_it_spans_vouched_or_not() {
    let (fs, dir) = ends_thinned();
    let mut log = open(&fs, &dir, NEVER, Trust::Unvouched);
    assert_eq!(log.end_offset(), offset(4));
    log.sync().unwrap();
    drop(log);
    // The walk of only the end of a vouched log has its own idea of where a batch ends.
    let mut log = open(&fs.process_restart(), &dir, NEVER, Trust::Vouched);
    assert_eq!(log.end_offset(), offset(4), "the span, not the one record");
    let at = log
        .append(&ones(1, 0), NOW, TimestampType::Create)
        .unwrap()
        .first_offset;
    assert_eq!(at, offset(4), "the next append goes after the span");
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 4]);
}

#[test]
fn an_empty_segment_beginning_inside_the_span_of_a_thinned_batch_is_a_leftover() {
    // A segment's end is found from its last batch's header. The empty one at 3 begins inside the offsets 1..=3 the
    // thinned batch spans, so it is a leftover of a failed roll and goes; were the thinned batch taken to end after
    // its one record (at 2), the segment would be taken for a successor and the log would end at 3, inside the batch.
    let (fs, dir) = ends_thinned();
    for ext in ["log", "index"] {
        fs.open_or_create(&dir.join(format!("{:020}.{ext}", 3)))
            .unwrap();
    }
    let mut log = reopen(&fs, &dir);
    assert_eq!(log.end_offset(), offset(4));
    assert_eq!(
        fs.names(&dir),
        vec![INDEX0, LOG0],
        "the leftover is removed"
    );
    let at = log
        .append(&ones(1, 0), NOW, TimestampType::Create)
        .unwrap()
        .first_offset;
    assert_eq!(at, offset(4));
}

#[test]
fn a_walk_of_the_batches_visits_a_thinned_last_batch_once() {
    let (fs, dir) = ends_thinned();
    let log = reopen(&fs, &dir);
    let mut seen = Vec::new();
    log.reader()
        .visit_batches(offset(0), |peek, _| seen.push(peek.base_offset))
        .unwrap();
    assert_eq!(seen, vec![0, 1]);
    // The same walk, for a producer's history.
    let history = log.reader().producer_history(u64::MAX).unwrap();
    let bases: Vec<_> = history.iter().map(|p| p.base_offset).collect();
    assert_eq!(bases, vec![1], "the producer's batch is there once");
}

#[test]
fn a_lookup_by_timestamp_goes_on_past_a_thinned_batch_that_ends_a_segment() {
    // Segments of 0 (a record, then the thinned batch, to offset 3) and 4 (a record). A lookup goes from batch to
    // batch, and from segment to segment by the offset after the last batch of one: taken short, it reads the thinned
    // batch again for ever. So it runs on a thread, and a hang is a failure that says so.
    let (fs, dir) = ends_thinned();
    put(&fs, &dir, "00000000000000000004.log", &raw(4, 1, 200));
    let log = reopen(&fs, &dir);
    assert_eq!(log.end_offset(), offset(5));
    let reader = log.reader();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let found = |ms| {
            reader
                .offset_for_timestamp(TimestampMs::from_wire(ms))
                .unwrap()
        };
        let _ = tx.send((found(101), found(150), found(201)));
    });
    let (inside, after, none) = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the lookup never got past the thinned batch");
    let got = |at: Option<(Offset, TimestampMs)>| at.map(|(o, t)| (o.get(), t.get()));
    assert_eq!(got(inside), Some((2, 101)), "the record left in it");
    assert_eq!(got(after), Some((4, 200)), "the one after its span");
    assert_eq!(got(none), None);
}
