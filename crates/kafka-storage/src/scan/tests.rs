use std::path::Path;

use kafka_wire::batch::build::BatchBuilder;

use super::*;
use crate::fs::{Fs, FsFile};
use crate::testutil::{disk, offset, raw, run};

/// A log file holding `bytes`, and its size.
fn log_of(bytes: &[u8]) -> (<crate::sim::SimFs as Fs>::File, u64) {
    let (fs, dir) = disk();
    let (file, _) = fs.open_or_create(&dir.join("0.log")).unwrap();
    file.write_all_at(bytes, 0).unwrap();
    (file, bytes.len() as u64)
}

fn walk(bytes: &[u8], base: i64, interval: u64, crc_from: u64) -> Scanned {
    let (file, size) = log_of(bytes);
    scan(&file, size, offset(base), interval, crc_from).unwrap()
}

#[test]
fn an_empty_log_ends_at_its_base() {
    let s = walk(&[], 40, 4096, 0);
    assert_eq!(
        (s.valid_end, s.next_offset, s.index.len()),
        (0, offset(40), 0)
    );
    assert_eq!(s.max_timestamp, None);
}

#[test]
fn a_clean_log_is_walked_to_its_end() {
    let log = run(10, &[2, 5, 1]);
    let s = walk(&log, 10, 1 << 20, 0);
    assert_eq!(s.valid_end, log.len() as u64);
    assert_eq!(s.next_offset, offset(18));
    assert_eq!(s.max_timestamp.map(|t| t.get()), Some(1_017));
    // One entry, for the first batch; the rest are within the interval.
    assert_eq!(
        s.index,
        vec![Entry {
            offset: 10,
            position: 0
        }]
    );
}

#[test]
fn the_index_gets_an_entry_each_time_the_interval_is_reached() {
    let one = raw(0, 1, 0).len() as u64;
    let log = run(0, &[1; 10]);
    let s = walk(&log, 0, 3 * one, 0);
    let at: Vec<_> = s
        .index
        .iter()
        .map(|e| (e.offset, e.position / one))
        .collect();
    // The first batch, then every third: an entry resets the count, so the batch it is for does not count.
    assert_eq!(at, vec![(0, 0), (3, 3), (6, 6), (9, 9)]);
    assert_eq!(s.since_index, 0);
}

#[test]
fn a_torn_tail_is_cut_at_the_last_whole_batch() {
    let log = run(0, &[1, 1, 1]);
    let one = log.len() / 3;
    for cut in [
        log.len() - 1,
        log.len() - 30,
        2 * one + 10,
        2 * one + 1,
        2 * one,
    ] {
        let s = walk(&log[..cut], 0, 4096, 0);
        assert_eq!(s.valid_end, 2 * one as u64, "cut at {cut}");
        assert_eq!(s.next_offset, offset(2));
    }
}

#[test]
fn a_zero_filled_tail_after_a_power_cut_is_cut() {
    let mut log = run(0, &[1, 1]);
    let whole = log.len() as u64;
    log.extend_from_slice(&[0; 10_000]);
    let s = walk(&log, 0, 4096, 0);
    assert_eq!((s.valid_end, s.next_offset), (whole, offset(2)));
}

#[test]
fn a_bad_checksum_is_found_only_where_checksums_are_checked() {
    let mut log = run(0, &[1, 1, 1]);
    let one = log.len() / 3;
    log[one + one / 2] ^= 0xff; // the middle batch's body
    // Every batch checked: the walk stops at the middle batch.
    assert_eq!(walk(&log, 0, 4096, 0).valid_end, one as u64);
    // Checked only from the last batch on: the damage in the middle is walked past.
    assert_eq!(
        walk(&log, 0, 4096, 2 * one as u64).valid_end,
        log.len() as u64
    );
    // A damaged last batch is found by the window that covers it.
    let mut log = run(0, &[1, 1, 1]);
    let last = log.len() - 3;
    log[last] ^= 0xff;
    assert_eq!(
        walk(&log, 0, 4096, 2 * one as u64).valid_end,
        2 * one as u64
    );
}

#[test]
fn a_batch_that_goes_backwards_in_offsets_ends_the_log() {
    let mut log = raw(0, 3, 0).to_vec();
    let stale = log.len();
    log.extend_from_slice(&raw(1, 1, 0)); // overlaps what came before: base 1 < next offset 3
    let s = walk(&log, 0, 4096, 0);
    assert_eq!((s.valid_end, s.next_offset), (stale as u64, offset(3)));
    // A gap (compaction leaves them) is fine.
    let mut gapped = raw(0, 3, 0).to_vec();
    gapped.extend_from_slice(&raw(10, 1, 0));
    assert_eq!(walk(&gapped, 0, 4096, 0).next_offset, offset(11));
}

#[test]
fn what_is_not_a_batch_ends_the_log() {
    let good = raw(0, 1, 0).len() as u64;
    let bad_header = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut log = run(0, &[1, 1]);
        let mut second = log.split_off(good as usize);
        f(&mut second);
        log.extend_from_slice(&second);
        walk(&log, 0, 4096, 0).valid_end
    };
    assert_eq!(bad_header(&|b| b[16] = 1), good, "not format v2");
    assert_eq!(
        bad_header(&|b| b[8..12].copy_from_slice(&5i32.to_be_bytes())),
        good,
        "length too small"
    );
    assert_eq!(
        bad_header(&|b| b[8..12].copy_from_slice(&i32::MAX.to_be_bytes())),
        good,
        "runs past the end"
    );
    assert_eq!(
        bad_header(&|b| b[57..61].copy_from_slice(&0i32.to_be_bytes())),
        good,
        "no records"
    );
    assert_eq!(
        bad_header(&|b| b[0..8].copy_from_slice(&i64::MAX.to_be_bytes())),
        good,
        "offsets overflow"
    );
}

#[test]
fn a_batch_bigger_than_the_read_window_is_walked() {
    let big = BatchBuilder::new()
        .base_offset(0)
        .records(1)
        .body(vec![0; 3 << 20])
        .build();
    let mut log = big.to_vec();
    log.extend_from_slice(&raw(1, 1, 0));
    let s = walk(&log, 0, 4096, 0);
    assert_eq!((s.valid_end, s.next_offset), (log.len() as u64, offset(2)));
}

#[test]
fn many_small_batches_cross_the_window_boundary_correctly() {
    let counts = vec![1u32; 20_000]; // about 1.4 MB: more than one window
    let log = run(0, &counts);
    assert!(log.len() > WINDOW);
    let s = walk(&log, 0, 1 << 30, 0);
    assert_eq!(
        (s.valid_end, s.next_offset),
        (log.len() as u64, offset(20_000))
    );
}

#[test]
fn the_newest_timestamp_is_read_from_headers_alone() {
    let (file, size) = log_of(&run(0, &[1, 1, 1]));
    assert_eq!(
        max_timestamp(&file, size).unwrap().map(|t| t.get()),
        Some(1_002)
    );
    let none = BatchBuilder::new().timestamps(-1, -1).build();
    let (file, size) = log_of(&none);
    assert_eq!(
        max_timestamp(&file, size).unwrap(),
        None,
        "-1 means no timestamp"
    );
    let _ = Path::new("");
}
