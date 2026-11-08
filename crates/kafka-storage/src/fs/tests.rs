//! The contract every [`Fs`] keeps, checked on the real file system and on the simulated one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::sim::SimFs;

/// A directory of its own on the real disk, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("kafka-storage-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn contract<F: Fs>(fs: &F, root: &Path) {
    files_read_and_write_by_position(fs, root);
    names_come_and_go(fs, root);
    a_durable_write_replaces_the_whole_file(fs, root);
}

fn files_read_and_write_by_position<F: Fs>(fs: &F, root: &Path) {
    let path = root.join("a.log");
    assert!(
        fs.open_existing(&path)
            .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
    );
    let (file, created) = fs.open_or_create(&path).unwrap();
    assert!(created);
    let (_, again) = fs.open_or_create(&path).unwrap();
    assert!(!again, "a file that is there is not created twice");

    file.write_all_at(b"hello", 0).unwrap();
    file.write_all_at(b"world", 5).unwrap();
    assert_eq!(file.size().unwrap(), 10);
    let mut buf = [0u8; 4];
    assert_eq!(file.read_at(&mut buf, 3).unwrap(), 4);
    assert_eq!(&buf, b"lowo");
    // Fewer bytes only at the end, and none past it.
    let mut buf = [0u8; 8];
    assert_eq!(file.read_at(&mut buf, 6).unwrap(), 4);
    assert_eq!(file.read_at(&mut buf, 10).unwrap(), 0);

    file.set_len(3).unwrap();
    assert_eq!(file.size().unwrap(), 3);
    file.set_len(6).unwrap();
    let mut all = [9u8; 6];
    assert_eq!(file.read_at(&mut all, 0).unwrap(), 6);
    assert_eq!(&all, b"hel\0\0\0", "growing a file reads back zeros");
    // A write past the end leaves a hole of zeros.
    file.write_all_at(b"x", 8).unwrap();
    assert_eq!(file.size().unwrap(), 9);
    file.sync_data().unwrap();
    assert!(file.modified_ms().unwrap() >= 0);
}

fn names_come_and_go<F: Fs>(fs: &F, root: &Path) {
    let dir = root.join("p-0");
    assert!(fs.create_dir(&dir).unwrap());
    assert!(
        !fs.create_dir(&dir).unwrap(),
        "a directory that is there is not made twice"
    );
    assert!(fs.create_dir(&dir.join("not/yet/there")).is_err());
    assert!(fs.create_dir_all(&dir.join("x/y")).unwrap());

    let (one, _) = fs.open_or_create(&dir.join("one")).unwrap();
    one.write_all_at(b"1", 0).unwrap();
    let (two, _) = fs.open_or_create(&dir.join("two")).unwrap();
    two.write_all_at(b"22", 0).unwrap();
    let mut names = fs.list_dir(&dir).unwrap();
    names.sort();
    let want = [("one", false), ("two", false), ("x", true)];
    let got: Vec<_> = names.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
    assert_eq!(got, want);

    // Renaming over a name replaces its file.
    fs.rename(&dir.join("one"), &dir.join("two")).unwrap();
    assert_eq!(
        read_file(fs, &dir.join("two")).unwrap(),
        Some(b"1".to_vec())
    );
    assert_eq!(read_file(fs, &dir.join("one")).unwrap(), None);
    assert!(fs.rename(&dir.join("one"), &dir.join("three")).is_err());

    assert!(fs.remove_file(&dir.join("two")).unwrap());
    assert!(
        !fs.remove_file(&dir.join("two")).unwrap(),
        "removing what is not there is not an error"
    );
    fs.sync_dir(&dir).unwrap();
    assert!(
        fs.list_dir(&root.join("nowhere"))
            .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
    );
}

fn a_durable_write_replaces_the_whole_file<F: Fs>(fs: &F, root: &Path) {
    let path = root.join("log-start");
    assert_eq!(read_file(fs, &path).unwrap(), None);
    write_durably(fs, &path, b"first value, which is long").unwrap();
    write_durably(fs, &path, b"2").unwrap();
    assert_eq!(
        read_file(fs, &path).unwrap(),
        Some(b"2".to_vec()),
        "no tail of the longer value"
    );
    let names: Vec<_> = fs
        .list_dir(root)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
}

#[test]
fn the_real_file_system_keeps_the_contract() {
    let scratch = Scratch::new();
    contract(&RealFs, &scratch.0);
}

#[test]
fn the_simulated_file_system_keeps_the_contract() {
    contract(&SimFs::new(), Path::new("/"));
}
