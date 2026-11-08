use std::path::Path;

use super::*;
use crate::fs::{Fs, FsFile};

const DIR: &str = "/p";

fn disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new(DIR)).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs
}

fn path(name: &str) -> PathBuf {
    Path::new(DIR).join(name)
}

fn cut(data: DataCut, names: NameCut, seed: u64) -> Cut {
    Cut { data, names, seed }
}

/// A file with `synced` flushed (and its name flushed) and `loose` written after.
fn file_with(fs: &SimFs, name: &str, synced: &[u8], loose: &[u8]) -> SimFile {
    let (file, _) = fs.open_or_create(&path(name)).unwrap();
    file.write_all_at(synced, 0).unwrap();
    file.sync_data().unwrap();
    fs.sync_dir(Path::new(DIR)).unwrap();
    file.write_all_at(loose, synced.len() as u64).unwrap();
    file
}

fn bytes(fs: &SimFs, name: &str) -> Option<Vec<u8>> {
    fs.contents(&path(name))
}

#[test]
fn a_cut_that_keeps_nothing_leaves_what_was_flushed() {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let after = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(bytes(&after, "a"), Some(b"safe".to_vec()));
}

#[test]
fn a_cut_that_keeps_everything_and_a_process_kill_lose_nothing() {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let kept = fs.power_cut(cut(DataCut::Keep, NameCut::Volatile, 1));
    assert_eq!(bytes(&kept, "a"), Some(b"safe-loose".to_vec()));
    assert_eq!(
        bytes(&fs.process_restart(), "a"),
        Some(b"safe-loose".to_vec())
    );
}

#[test]
fn unflushed_writes_may_read_back_as_zeros() {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let after = fs.power_cut(cut(DataCut::Zero, NameCut::Durable, 1));
    assert_eq!(bytes(&after, "a"), Some(b"safe\0\0\0\0\0\0".to_vec()));
}

#[test]
fn a_prefix_cut_keeps_the_flushed_bytes_and_some_of_the_rest() {
    let loose: Vec<u8> = (0..200u8).collect();
    for seed in 0..40 {
        let fs = disk();
        let _file = file_with(&fs, "a", b"safe", &loose);
        let after = bytes(
            &fs.power_cut(cut(DataCut::Prefix, NameCut::Durable, seed)),
            "a",
        )
        .unwrap();
        assert!(after.starts_with(b"safe"), "seed {seed}");
        assert!(after.len() <= 4 + loose.len(), "seed {seed}");
        assert_eq!(
            after.get(4..),
            loose.get(..after.len() - 4),
            "a prefix, byte for byte (seed {seed})"
        );
    }
}

#[test]
fn a_page_cut_keeps_the_flushed_bytes_when_it_only_appends() {
    let loose = vec![7u8; 3 * 4096];
    for seed in 0..40 {
        let fs = disk();
        let _file = file_with(&fs, "a", b"safe", &loose);
        let after = bytes(
            &fs.power_cut(cut(DataCut::Pages, NameCut::Durable, seed)),
            "a",
        )
        .unwrap();
        assert!(after.starts_with(b"safe"), "seed {seed}");
        assert!(
            after.iter().skip(4).all(|b| *b == 7 || *b == 0),
            "pages are whole or absent (seed {seed})"
        );
    }
}

#[test]
fn names_follow_the_directory_flush() {
    let fs = disk();
    let (made, _) = fs.open_or_create(&path("new")).unwrap();
    made.write_all_at(b"data", 0).unwrap();
    made.sync_data().unwrap(); // the contents are flushed; the name is not
    let gone = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(
        bytes(&gone, "new"),
        None,
        "a flushed file vanishes with its unflushed name"
    );
    fs.sync_dir(Path::new(DIR)).unwrap();
    let kept = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(bytes(&kept, "new"), Some(b"data".to_vec()));
}

#[test]
fn an_unflushed_removal_and_rename_are_undone() {
    let fs = disk();
    let _a = file_with(&fs, "a", b"A", b"");
    let _b = file_with(&fs, "b", b"B", b"");
    fs.remove_file(&path("a")).unwrap();
    fs.rename(&path("b"), &path("c")).unwrap();
    let after = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(
        bytes(&after, "a"),
        Some(b"A".to_vec()),
        "the removed file is back"
    );
    assert_eq!(bytes(&after, "b"), Some(b"B".to_vec()));
    assert_eq!(bytes(&after, "c"), None);
    let all = fs.power_cut(cut(DataCut::Drop, NameCut::Volatile, 1));
    assert_eq!(
        (bytes(&all, "a"), bytes(&all, "c")),
        (None, Some(b"B".to_vec()))
    );
}

#[test]
fn random_name_cuts_are_replayable_and_apply_changes_in_order() {
    let make = || {
        let fs = disk();
        for i in 0..6 {
            let _ = file_with(&fs, &format!("f{i}"), b"x", b"");
            fs.rename(&path(&format!("f{i}")), &path(&format!("g{i}")))
                .unwrap();
        }
        fs
    };
    let names = |seed| {
        make()
            .power_cut(cut(DataCut::Drop, NameCut::Random, seed))
            .names(Path::new(DIR))
    };
    assert_eq!(names(5), names(5));
    assert!(
        (0..20)
            .map(names)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1
    );
    // A name is either its old or its new, never both and never neither.
    for seed in 0..20 {
        let seen = names(seed);
        for i in 0..6 {
            let (old, new) = (format!("f{i}"), format!("g{i}"));
            assert!(
                seen.contains(&old) ^ seen.contains(&new),
                "seed {seed}: {seen:?}"
            );
        }
    }
}

#[test]
fn the_nth_operation_of_a_kind_fails_once() {
    let fs = disk();
    let (file, _) = fs.open_or_create(&path("a")).unwrap();
    fs.fail(Op::Write, 2, Failure::Io);
    file.write_all_at(b"1", 0).unwrap();
    assert!(file.write_all_at(b"2", 1).is_err());
    file.write_all_at(b"3", 1).unwrap();
    assert_eq!(
        bytes(&fs, "a"),
        Some(b"13".to_vec()),
        "the failed write left nothing"
    );
}

#[test]
fn the_nth_operation_of_any_kind_fails_and_the_trace_names_it() {
    let fs = disk();
    let before = fs.ops();
    fs.fail_any(3, Failure::Io);
    let (file, _) = fs.open_or_create(&path("a")).unwrap(); // 1: create
    file.write_all_at(b"1", 0).unwrap(); // 2: write
    assert!(file.sync_data().is_err()); // 3: fsync
    let trace = fs.trace();
    assert_eq!(trace.len() as u64, fs.ops());
    assert_eq!(trace.get(before as usize + 2).map(|t| t.0), Some(Op::Fsync));
}

#[test]
fn the_process_can_be_stopped_after_any_operation() {
    let fs = disk();
    let (file, _) = fs.open_or_create(&path("a")).unwrap();
    fs.halt_after(2);
    file.write_all_at(b"1", 0).unwrap();
    file.write_all_at(b"2", 1).unwrap();
    assert!(file.write_all_at(b"3", 2).is_err());
    assert_eq!(bytes(&fs, "a"), Some(b"12".to_vec()));
    assert!(
        fs.open_existing(&path("a")).is_err(),
        "a stopped process opens nothing"
    );
}

#[test]
fn a_full_disk_cuts_a_write_short_and_fails_it() {
    let fs = disk();
    let (file, _) = fs.open_or_create(&path("a")).unwrap();
    fs.set_capacity(Some(6));
    file.write_all_at(b"abcd", 0).unwrap();
    let err = file.write_all_at(b"efgh", 4).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::StorageFull);
    assert_eq!(
        bytes(&fs, "a"),
        Some(b"abcdef".to_vec()),
        "the part that fitted was written"
    );
    file.set_len(4).unwrap();
    file.write_all_at(b"ef", 4).unwrap();
    fs.set_capacity(None);
    file.write_all_at(b"gh", 6).unwrap();
}

#[test]
fn a_failed_fsync_with_lost_pages_makes_a_retry_succeed_without_making_anything_durable() {
    let fs = disk();
    fs.lose_pages_on_failed_fsync(true);
    let file = file_with(&fs, "a", b"safe", b"-loose");
    fs.fail(Op::Fsync, 1, Failure::Io);
    assert!(file.sync_data().is_err());
    file.sync_data().unwrap(); // the retry that proves nothing
    assert_eq!(
        bytes(&fs, "a"),
        Some(b"safe-loose".to_vec()),
        "still readable"
    );
    let after = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(
        bytes(&after, "a"),
        Some(b"safe".to_vec()),
        "never made durable"
    );
}

#[test]
fn a_cut_starts_a_new_process_with_a_clean_slate() {
    let fs = disk();
    fs.halt_after(0);
    let after = fs.power_cut(cut(DataCut::Keep, NameCut::Volatile, 1));
    assert!(
        after.open_or_create(&path("fresh")).is_ok(),
        "faults and the halt do not carry over"
    );
}

#[test]
fn relative_paths_are_refused() {
    assert!(SimFs::new().open_existing(Path::new("a/b")).is_err());
}
