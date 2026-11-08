use super::*;
use crate::sim::{Op, SimFs};
use crate::testutil::{batch, disk, offset, raw};

const INTERVAL: u64 = 4096;

fn active(fs: &SimFs, dir: &std::path::Path, base: i64, interval: u64) -> ActiveSegment<SimFs> {
    ActiveSegment::open(fs, dir, offset(base), interval)
        .unwrap()
        .0
}

fn one() -> u64 {
    raw(0, 1, 0).len() as u64
}

#[test]
fn names_are_twenty_digits_and_round_trip() {
    let dir = std::path::Path::new("/d");
    assert_eq!(
        log_path(dir, offset(7)).to_str(),
        Some("/d/00000000000000000007.log")
    );
    assert_eq!(
        index_path(dir, offset(7)).to_str(),
        Some("/d/00000000000000000007.index")
    );
    assert_eq!(base_of_log("00000000000000000042.log"), Some(offset(42)));
    assert_eq!(
        base_of_index("00000000000000000042.index"),
        Some(offset(42))
    );
    assert_eq!(base_of_log("42.index"), None);
    assert_eq!(base_of_log("log-start"), None);
    assert_eq!(base_of_log("-5.log"), None, "an offset is not negative");
    for loose in [
        "5.log",
        "+5.log",
        "007.log",
        "0000000000000000005.log",
        "000000000000000000005.log",
        "+0000000000000000005.log",
    ] {
        assert_eq!(
            base_of_log(loose),
            None,
            "{loose}: not the canonical twenty digits"
        );
    }
    assert_eq!(base_of_log("00000000000000000005.log"), Some(offset(5)));
    assert_eq!(base_of_log("abc.log"), None);
}

#[test]
fn the_index_gets_the_first_batch_and_then_one_per_interval() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 10, 2 * one());
    for i in 0..5 {
        seg.append(&batch(1, 0).stamped(offset(10 + i), None), 0)
            .unwrap();
    }
    seg.publish();
    let at = |offset, batches| crate::index::Entry {
        offset,
        position: batches * one(),
    };
    let index = seg.view().index().clone();
    assert!(index.matches(&[at(10, 0), at(12, 2), at(14, 4)]).unwrap());
    assert!(!index.matches(&[at(10, 0), at(12, 2)]).unwrap(), "fewer");
    assert!(
        !index.matches(&[at(10, 0), at(12, 2), at(14, 5)]).unwrap(),
        "different"
    );
    let view = seg.view();
    assert_eq!(
        view.seek(9).unwrap(),
        0,
        "below every entry: from the start"
    );
    assert_eq!(view.seek(11).unwrap(), 0);
    assert_eq!(view.seek(12).unwrap(), 2 * one());
    assert_eq!(view.seek(13).unwrap(), 2 * one());
    assert_eq!(view.seek(1_000).unwrap(), 4 * one());
}

#[test]
fn sync_flushes_only_what_changed() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    let fsyncs = |fs: &SimFs| fs.trace().iter().filter(|(op, _)| *op == Op::Fsync).count();
    seg.sync().unwrap();
    assert_eq!(fsyncs(&fs), 0, "nothing written: no system call");
    seg.append(&batch(1, 0).stamped(offset(0), None), 1)
        .unwrap();
    assert!(seg.is_dirty());
    seg.sync().unwrap();
    assert_eq!(
        fsyncs(&fs),
        2,
        "the log and the index it gained an entry in"
    );
    seg.append(&batch(1, 0).stamped(offset(1), None), 1)
        .unwrap(); // no new index entry
    seg.sync().unwrap();
    assert_eq!(fsyncs(&fs), 3, "only the log");
    seg.sync().unwrap();
    assert_eq!(fsyncs(&fs), 3);
    assert!(!seg.is_dirty());
}
