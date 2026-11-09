//! A broker that stops while a round is being written: the round gives up between two batches of the walk that writes the
//! new segments, not only at its start (where the walk that reads the keys would see it).

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::compaction::{PARAMS, kv};
use super::*;
use crate::fs::{DirEntry, Fs, FsFile};
use crate::sim::SimFile;

/// A disk that sets a flag at the `n`th write after it is armed, as a broker that is stopping would.
#[derive(Clone)]
struct Tripwire {
    disk: SimFs,
    writes_left: Arc<AtomicU64>,
    flag: Arc<AtomicBool>,
}

struct TripFile {
    file: SimFile,
    trip: Tripwire,
}

impl Fs for Tripwire {
    type File = TripFile;

    fn open_existing(&self, path: &Path) -> io::Result<TripFile> {
        let file = self.disk.open_existing(path)?;
        Ok(TripFile {
            file,
            trip: self.clone(),
        })
    }

    fn open_or_create(&self, path: &Path) -> io::Result<(TripFile, bool)> {
        let (file, made) = self.disk.open_or_create(path)?;
        let file = TripFile {
            file,
            trip: self.clone(),
        };
        Ok((file, made))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.disk.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        self.disk.remove_file(path)
    }

    fn create_dir(&self, path: &Path) -> io::Result<bool> {
        self.disk.create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<bool> {
        self.disk.remove_dir(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        self.disk.list_dir(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.disk.sync_dir(path)
    }
}

impl FsFile for TripFile {
    fn read_at(&self, buf: &mut [u8], pos: u64) -> io::Result<usize> {
        self.file.read_at(buf, pos)
    }

    fn write_all_at(&self, data: &[u8], pos: u64) -> io::Result<()> {
        let left = self.trip.writes_left.load(Ordering::SeqCst);
        if left > 0 {
            self.trip.writes_left.store(left - 1, Ordering::SeqCst);
            if left == 1 {
                self.trip.flag.store(true, Ordering::SeqCst);
            }
        }
        self.file.write_all_at(data, pos)
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        self.file.set_len(len)
    }

    fn size(&self) -> io::Result<u64> {
        self.file.size()
    }

    fn sync_data(&self) -> io::Result<()> {
        self.file.sync_data()
    }

    fn modified_ms(&self) -> io::Result<i64> {
        self.file.modified_ms()
    }
}

/// Every file of the directory and what it holds.
fn contents(disk: &SimFs, dir: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    let mut all: Vec<_> = disk
        .names(dir)
        .into_iter()
        .map(|n| {
            let bytes = disk.contents(&dir.join(&n));
            (n, bytes)
        })
        .collect();
    all.sort();
    all
}

#[test]
fn a_round_told_to_stop_while_it_writes_gives_up_and_leaves_no_files_and_the_log_as_it_was() {
    let seg = kv("a", Some("v")).len() as u64;
    let (disk, dir) = disk();
    let trip = Tripwire {
        disk: disk.clone(),
        writes_left: Arc::new(AtomicU64::new(0)),
        flag: Arc::new(AtomicBool::new(false)),
    };
    let mut log =
        PartitionLog::open(trip.clone(), dir.clone(), config(3 * seg), Trust::Unvouched).unwrap();
    for i in 0..30 {
        let batch = kv(&format!("k{}", i % 3), Some("v"));
        log.append(&[batch], NOW, TimestampType::Create).unwrap();
    }
    log.sync().unwrap();
    let before = contents(&disk, &dir);
    let job = log
        .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
        .unwrap()
        .expect("due");
    // The flag is clear through the walk that reads the keys; it is set by the first write of the new segments, with the
    // rest of the log still to be gone through.
    trip.writes_left.store(1, Ordering::SeqCst);
    let outcome = job.run_until(&trip.flag);
    assert!(trip.flag.load(Ordering::SeqCst), "the round wrote nothing");
    match outcome {
        Err(crate::StorageError::Io { source, .. }) => {
            assert_eq!(source.kind(), io::ErrorKind::Interrupted);
        }
        other => panic!("not interrupted: {other:?}"),
    }
    let after = contents(&disk, &dir);
    assert!(
        after.iter().all(|(n, _)| !n.ends_with(".tmp")),
        "a stopped round leaves nothing: {after:?}"
    );
    assert_eq!(after, before, "the log is as it was");
}
