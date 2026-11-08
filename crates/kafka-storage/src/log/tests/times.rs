use super::*;
use crate::testutil::offset;
use bytes::Bytes;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::ids::TimestampMs;

fn at(ms: i64) -> TimestampMs {
    TimestampMs::from_wire(ms)
}

/// A batch of `records` records that were all written at `ts`.
fn written_at(records: u32, ts: i64) -> Batch {
    batch(records, ts)
}

fn zigzag(n: i64) -> Vec<u8> {
    let mut v = ((n << 1) ^ (n >> 63)) as u64;
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// A batch whose records were written `first + delta` for each delta, in the order given.
fn spread(first: i64, deltas: &[i64]) -> Batch {
    let mut body = Vec::new();
    for (i, delta) in deltas.iter().enumerate() {
        let mut record = vec![0u8];
        record.extend(zigzag(*delta));
        record.extend(zigzag(i as i64));
        record.extend(zigzag(-1)); // null key
        record.extend(zigzag(-1)); // null value
        record.extend(zigzag(0)); // no headers
        body.extend(zigzag(record.len() as i64));
        body.extend(record);
    }
    let max = first + deltas.iter().max().unwrap();
    let bytes: Bytes = BatchBuilder::new()
        .records(deltas.len() as u32)
        .timestamps(first, max)
        .body(body)
        .build();
    Batch::parse(bytes).unwrap()
}

#[test]
fn a_record_is_found_by_when_it_was_written_down_to_the_record_and_not_the_batch() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    // Written at 1000, 1050 and 1020 (a producer's records need not be in time order).
    log.append(&[spread(1_000, &[0, 50, 20])], NOW, TimestampType::Create)
        .unwrap();
    log.append(&[written_at(2, 2_000)], NOW, TimestampType::Create)
        .unwrap();
    let r = log.reader();
    assert_eq!(
        r.offset_for_timestamp(at(0)).unwrap(),
        Some((offset(0), at(1_000)))
    );
    assert_eq!(
        r.offset_for_timestamp(at(1_030)).unwrap(),
        Some((offset(1), at(1_050))),
        "the record, not the batch"
    );
    assert_eq!(
        r.offset_for_timestamp(at(1_051)).unwrap(),
        Some((offset(3), at(2_000)))
    );
    assert_eq!(r.offset_for_timestamp(at(2_001)).unwrap(), None);
    assert_eq!(
        r.offset_of_max_timestamp().unwrap(),
        Some((offset(3), at(2_000))),
        "the first of the batch's two"
    );
}

#[test]
fn an_empty_log_has_no_record_at_any_time() {
    let (_fs, _dir, log) = fresh(NEVER);
    assert_eq!(log.reader().offset_for_timestamp(at(0)).unwrap(), None);
    assert_eq!(log.reader().offset_of_max_timestamp().unwrap(), None);
}

#[test]
fn records_below_the_log_start_are_not_found_even_inside_the_batch_that_holds_it() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    log.append(
        &[spread(1_000, &[0, 10, 20, 30])],
        NOW,
        TimestampType::Create,
    )
    .unwrap();
    log.delete_before(Some(offset(2))).unwrap();
    let r = log.reader();
    assert_eq!(
        r.offset_for_timestamp(at(0)).unwrap(),
        Some((offset(2), at(1_020)))
    );
    assert_eq!(
        r.offset_of_max_timestamp().unwrap(),
        Some((offset(3), at(1_030)))
    );
}

/// Against a list of every record: random timestamps (not in order), batches of several sizes across several
/// segments, a log start moved into the middle.
#[test]
fn lookups_agree_with_a_list_of_every_record() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    for _ in 0..40 {
        let (_fs, _dir, mut log) = fresh(300);
        let mut records: Vec<(i64, i64)> = Vec::new(); // (offset, timestamp)
        for _ in 0..(1 + next(12)) {
            let (count, ts) = (1 + next(3) as u32, 1_000 + next(50) as i64);
            let first = records.len() as i64;
            log.append(&[written_at(count, ts)], NOW, TimestampType::Create)
                .unwrap();
            records.extend((0..i64::from(count)).map(|i| (first + i, ts)));
        }
        let start = next(records.len() as u64 + 1) as i64;
        log.delete_before(Some(offset(start))).unwrap();
        let visible: Vec<_> = records
            .iter()
            .filter(|(o, _)| *o >= log.start_offset().get())
            .collect();
        let r = log.reader();
        for target in [0, 1_000, 1_010, 1_025, 1_049, 1_050, 1_051] {
            let want = visible
                .iter()
                .find(|(_, t)| *t >= target)
                .map(|(o, t)| (offset(*o), at(*t)));
            assert_eq!(
                r.offset_for_timestamp(at(target)).unwrap(),
                want,
                "target {target}"
            );
        }
        let newest = visible.iter().map(|(_, t)| *t).max();
        let want = newest
            .and_then(|n| visible.iter().find(|(_, t)| *t == n))
            .map(|(o, t)| (offset(*o), at(*t)));
        assert_eq!(r.offset_of_max_timestamp().unwrap(), want);
    }
}

#[test]
fn a_compressed_batch_answers_at_the_grain_of_its_header_and_is_skipped_when_too_old() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    // Its records are inside the codec, so only the header's newest timestamp can be looked at.
    let gzip = Batch::parse(
        BatchBuilder::new()
            .records(3)
            .attributes(1)
            .timestamps(1_000, 1_000)
            .build(),
    )
    .unwrap();
    log.append(&[gzip], NOW, TimestampType::Create).unwrap();
    log.append(&[written_at(2, 2_000)], NOW, TimestampType::Create)
        .unwrap();
    let r = log.reader();
    assert_eq!(
        r.offset_for_timestamp(at(500)).unwrap(),
        Some((offset(0), at(1_000)))
    );
    assert_eq!(
        r.offset_for_timestamp(at(1_500)).unwrap(),
        Some((offset(3), at(2_000))),
        "too old to hold it: skipped"
    );
}
