use std::path::Path;

use super::*;
use crate::fs::{Fs, FsFile};
use crate::sim::{Cut, DataCut, NameCut, SimFs};

const DATA: &str = "/data";

fn disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new(DATA)).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs
}

fn marker(fs: &SimFs) -> Option<Vec<u8>> {
    fs.contents(&Path::new(DATA).join(FILE))
}

fn write_raw(fs: &SimFs, bytes: &[u8]) {
    let (file, _) = fs.open_or_create(&Path::new(DATA).join(FILE)).unwrap();
    file.write_all_at(bytes, 0).unwrap();
}

#[test]
fn with_no_marker_nothing_is_vouched_for() {
    let state = StopState::begin(&disk(), Path::new(DATA)).unwrap();
    assert!(!state.was_clean());
    assert_eq!(state.trust("orders-0"), Trust::Unvouched);
}

#[test]
fn a_clean_stop_vouches_for_every_partition_it_does_not_name() {
    let fs = disk();
    record_clean_stop(&fs, Path::new(DATA), &["slow-0".into(), "slow-1".into()]).unwrap();
    let state = StopState::begin(&fs, Path::new(DATA)).unwrap();
    assert!(state.was_clean());
    assert_eq!(state.trust("orders-0"), Trust::Vouched);
    assert_eq!(
        state.trust("slow-0"),
        Trust::Unvouched,
        "never opened since an unclean stop"
    );
    assert_eq!(
        state.not_covered().collect::<Vec<_>>(),
        vec!["slow-0", "slow-1"]
    );
}

#[test]
fn the_marker_is_gone_for_good_before_anything_else_is_written() {
    let fs = disk();
    record_clean_stop(&fs, Path::new(DATA), &[]).unwrap();
    assert!(marker(&fs).is_some());
    StopState::begin(&fs, Path::new(DATA)).unwrap();
    assert!(marker(&fs).is_none());
    // A marker that came back after a power cut would vouch for a log this run was in the middle of writing.
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert!(marker(&after).is_none());
    assert!(
        !StopState::begin(&after, Path::new(DATA))
            .unwrap()
            .was_clean()
    );
}

#[test]
fn a_recorded_stop_survives_a_power_cut() {
    let fs = disk();
    record_clean_stop(&fs, Path::new(DATA), &[]).unwrap();
    let after = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert!(
        StopState::begin(&after, Path::new(DATA))
            .unwrap()
            .was_clean()
    );
}

#[test]
fn a_marker_that_is_not_whole_vouches_for_nothing() {
    for bytes in [
        &b"doze-kafka clean shutdown 1"[..],    // no newline: torn
        b"doze-kafka clean shutdown 1\nslow-0", // the last name is torn
        b"something else\n",                    // not ours
        b"",                                    // the name reached the disk before its bytes
        &[0xff, 0xfe, b'\n'],                   // not text
    ] {
        let fs = disk();
        write_raw(&fs, bytes);
        let state = StopState::begin(&fs, Path::new(DATA)).unwrap();
        assert!(!state.was_clean(), "{bytes:?}");
        assert_eq!(state.trust("orders-0"), Trust::Unvouched);
        assert!(marker(&fs).is_none(), "and it is removed all the same");
    }
}
