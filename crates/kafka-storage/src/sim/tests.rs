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
fn relative_paths_are_refused() {
    assert!(SimFs::new().open_existing(Path::new("a/b")).is_err());
}

mod cuts;
