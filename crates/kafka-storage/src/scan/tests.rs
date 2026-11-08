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
