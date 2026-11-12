//! The metadata database holds the SCRAM keys and salts, so only its owner may read it (SEC-193).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use kafka_meta::{FileMode, MetaStore};
use kafka_storage::fs::{Fs as _, RealFs};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kafka-meta-mode-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_database_is_made_readable_by_its_owner_only() {
    // Made private, not made and then narrowed: the file is never open to others, not for a moment.
    let dir = scratch("new");
    let store = MetaStore::open(&RealFs, &dir).unwrap();
    assert_eq!(store.file_mode(), FileMode::Private);
    drop(store);
    assert_eq!(mode(&dir.join("meta.redb")), 0o600);
    assert!(!dir.join("meta.redb.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_database_an_earlier_build_made_open_is_narrowed_when_it_is_opened() {
    let dir = scratch("old");
    drop(MetaStore::open(&RealFs, &dir).unwrap());
    let path = dir.join("meta.redb");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let store = MetaStore::open(&RealFs, &dir).unwrap();
    assert_eq!(store.file_mode(), FileMode::Narrowed);
    drop(store);
    assert_eq!(mode(&path), 0o600);
    // Opened again, there is nothing to narrow.
    let store = MetaStore::open(&RealFs, &dir).unwrap();
    assert_eq!(store.file_mode(), FileMode::Private);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_database_found_with_any_group_or_other_bit_is_narrowed_and_an_owner_only_one_is_left() {
    // SEC-237: each of the group and other bits an owner who can still open the file may have set, not only the 0644 and
    // 0664 of the ordinary umasks.
    let dir = scratch("modes");
    drop(MetaStore::open(&RealFs, &dir).unwrap());
    let path = dir.join("meta.redb");
    for (found, narrowed) in [
        (0o640, true),
        (0o604, true),
        (0o660, true),
        (0o606, true),
        (0o600, false),
        (0o700, false),
    ] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(found)).unwrap();
        let store = MetaStore::open(&RealFs, &dir).unwrap();
        let want = if narrowed {
            FileMode::Narrowed
        } else {
            FileMode::Private
        };
        assert_eq!(store.file_mode(), want, "{found:o}");
        drop(store);
        assert_eq!(
            mode(&path),
            if narrowed { 0o600 } else { found },
            "{found:o}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file system that cannot take access away from a file (it is not ours, say): everything else is the simulated disk's.
#[derive(Clone)]
struct Unchangeable(kafka_storage::sim::SimFs);

impl kafka_storage::fs::Fs for Unchangeable {
    type File = <kafka_storage::sim::SimFs as kafka_storage::fs::Fs>::File;
    fn open_existing(&self, p: &Path) -> std::io::Result<Self::File> {
        self.0.open_existing(p)
    }
    fn open_or_create(&self, p: &Path) -> std::io::Result<(Self::File, bool)> {
        self.0.open_or_create(p)
    }
    fn restrict_to_owner(&self, _: &Path) -> std::io::Result<bool> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
    fn rename(&self, a: &Path, b: &Path) -> std::io::Result<()> {
        self.0.rename(a, b)
    }
    fn remove_file(&self, p: &Path) -> std::io::Result<bool> {
        self.0.remove_file(p)
    }
    fn create_dir(&self, p: &Path) -> std::io::Result<bool> {
        self.0.create_dir(p)
    }
    fn remove_dir(&self, p: &Path) -> std::io::Result<bool> {
        self.0.remove_dir(p)
    }
    fn list_dir(&self, p: &Path) -> std::io::Result<Vec<kafka_storage::fs::DirEntry>> {
        self.0.list_dir(p)
    }
    fn sync_dir(&self, p: &Path) -> std::io::Result<()> {
        self.0.sync_dir(p)
    }
}

#[test]
fn a_database_whose_access_cannot_be_taken_away_is_reported_wide() {
    let fs = Unchangeable(kafka_storage::sim::SimFs::new());
    fs.create_dir(Path::new("/")).ok();
    let store = MetaStore::open(&fs, Path::new("/")).unwrap();
    assert_eq!(store.file_mode(), FileMode::Wide);
    // And it is not the answer of a disk that has no modes: that one is private.
    let plain = kafka_storage::sim::SimFs::new();
    plain.create_dir(Path::new("/")).ok();
    assert_eq!(
        MetaStore::open(&plain, Path::new("/")).unwrap().file_mode(),
        FileMode::Private
    );
}
