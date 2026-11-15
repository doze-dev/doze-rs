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

/// `spread`, its records compressed with lz4 as a producer would send them.
fn lz4(first: i64, deltas: &[i64]) -> Batch {
    let plain = spread(first, deltas);
    let newest = plain.max_timestamp();
    plain
        .recode(kafka_wire::batch::Compression::Lz4, 1 << 20, newest)
        .unwrap()
}

#[test]
fn a_compressed_batch_is_inflated_to_find_the_record_as_kafka_does() {
    // Kafka's FileRecords.searchForTimestamp and RecordBatch.offsetOfMaxTimestamp iterate the records of a compressed
    // batch; the audit's probe recorded +250 -> offset 3 at +300, +400 and -3 -> offset 4 at +400.
    let (_fs, _dir, mut log) = fresh(NEVER);
    log.append(
        &[lz4(10_000, &[0, 100, 200, 300, 400])],
        NOW,
        TimestampType::Create,
    )
    .unwrap();
    let r = log.reader();
    assert_eq!(
        r.offset_for_timestamp(at(10_250)).unwrap(),
        Some((offset(3), at(10_300)))
    );
    assert_eq!(
        r.offset_for_timestamp(at(10_400)).unwrap(),
        Some((offset(4), at(10_400)))
    );
    assert_eq!(
        r.offset_of_max_timestamp().unwrap(),
        Some((offset(4), at(10_400)))
    );
    // Out of order inside the batch: the first record at the newest time, not the batch's first.
    log.append(&[lz4(20_000, &[0, 50, 20])], NOW, TimestampType::Create)
        .unwrap();
    let r = log.reader();
    assert_eq!(
        r.offset_of_max_timestamp().unwrap(),
        Some((offset(6), at(20_050)))
    );
    assert_eq!(
        r.offset_for_timestamp(at(20_010)).unwrap(),
        Some((offset(6), at(20_050)))
    );
}

#[test]
fn under_log_append_time_every_record_is_at_its_batchs_append_time() {
    // The audit's probe against Kafka: two batches the producer stamped 1000, the broker at A1 and A2. Kafka answers
    // A1-1 and A1 with offset 0 at A1, A1+1 with offset 2 at A2, -3 with 2 at A2, and 1000 with 0 at A1.
    let (_fs, _dir, mut log) = fresh(NEVER);
    let (a1, a2) = (NOW.get(), NOW.get() + 34);
    log.append(&[spread(1_000, &[0, 5])], at(a1), TimestampType::LogAppend)
        .unwrap();
    log.append(&[spread(1_000, &[0])], at(a2), TimestampType::LogAppend)
        .unwrap();
    let r = log.reader();
    for (target, want) in [
        (a1 - 1, Some((offset(0), at(a1)))),
        (a1, Some((offset(0), at(a1)))),
        (a1 + 1, Some((offset(2), at(a2)))),
        (1_000, Some((offset(0), at(a1)))),
        (a2 + 1, None),
    ] {
        assert_eq!(
            r.offset_for_timestamp(at(target)).unwrap(),
            want,
            "{target}"
        );
    }
    assert_eq!(
        r.offset_of_max_timestamp().unwrap(),
        Some((offset(2), at(a2)))
    );
}

#[test]
fn a_batch_whose_records_cannot_be_read_answers_at_the_grain_of_its_header_and_is_skipped_when_too_old()
 {
    let (_fs, _dir, mut log) = fresh(NEVER);
    // It says gzip and is not: its records cannot be walked, so only the header's newest timestamp can be looked at.
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
