//! The metadata database holds the SCRAM keys and salts, so only its owner may read it (SEC-193).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use kafka_meta::{FileMode, MetaStore};
use kafka_storage::fs::RealFs;

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
