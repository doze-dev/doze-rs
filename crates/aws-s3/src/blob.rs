//! Object bytes on disk, crash-safe. A body is written to `blobs/<run>.<n>.part`, flushed, and only then given its name by a rename
//! (and the directory flushed): so a power cut leaves a whole blob or none, and a `.part` file left behind is removed at the next
//! start. The metadata that points at a blob is committed after the blob is named, so a crash between the two leaves an unreferenced
//! blob (swept when the service starts after an unclean stop), never metadata that points at nothing.
//!
//! Every disk operation goes through [`Fs`], so the crash tests run on the simulated disk.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use doze_fs::fs::{Fs, FsFile};

const DIR: &str = "blobs";
const PART: &str = ".part";

/// The blobs of one region.
#[derive(Debug)]
pub(crate) struct Blobs<F: Fs> {
    fs: F,
    dir: PathBuf,
    /// Names this process's staging files, so a stale one from an earlier run can be told from one in use.
    run: String,
    next: AtomicU64,
}

/// A body being written.
pub(crate) struct Staging<F: Fs> {
    pub(crate) file: Arc<F::File>,
    path: PathBuf,
    /// Bytes written so far.
    pub(crate) len: u64,
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    // WHY: with no randomness the name is all zeros; the counter still keeps staging names apart within a run, and blob ids are
    // checked for a collision before use.
    if getrandom::fill(&mut b).is_err() {
        b.fill(0);
    }
    aws_core::hex::encode(&b)
}

impl<F: Fs> Blobs<F> {
    pub(crate) fn new(fs: F, region_dir: &Path) -> Self {
        static RUN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        Self::with_run(fs, region_dir, RUN.get_or_init(|| random_hex(6)).clone())
    }

    /// Blobs under an explicit run name (a test plays two runs).
    pub(crate) fn with_run(fs: F, region_dir: &Path, run: String) -> Self {
        Self {
            fs,
            dir: region_dir.join(DIR),
            run,
            next: AtomicU64::new(0),
        }
    }

    /// Starts a body.
    ///
    /// # Errors
    /// The disk refused.
    pub(crate) fn begin(&self) -> io::Result<Staging<F>> {
        doze_fs::fs::create_dirs_durably(&self.fs, &self.dir)?;
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.join(format!("{}.{n}{PART}", self.run));
        let (file, _) = self.fs.open_or_create(&path)?;
        file.set_len(0)?;
        Ok(Staging {
            file: Arc::new(file),
            path,
            len: 0,
        })
    }

    /// Flushes a finished body and gives it its name: the blob's id.
    ///
    /// # Errors
    /// The disk refused; the staging file is removed.
    pub(crate) fn publish(&self, staged: &Staging<F>) -> io::Result<String> {
        let result = self.publish_inner(staged);
        if result.is_err() {
            self.discard(staged);
        }
        result
    }

    fn publish_inner(&self, staged: &Staging<F>) -> io::Result<String> {
        staged.file.sync_data()?;
        let mut id = random_hex(16);
        while self.fs.open_existing(&self.dir.join(&id)).is_ok() {
            id = random_hex(16);
        }
        self.fs.rename(&staged.path, &self.dir.join(&id))?;
        self.fs.sync_dir(&self.dir)?;
        Ok(id)
    }

    /// Throws a staging file away.
    pub(crate) fn discard(&self, staged: &Staging<F>) {
        // WHY: a file that cannot be removed now is removed by the sweep at the next start.
        if self.fs.remove_file(&staged.path).is_err() {
            return;
        }
    }

    /// Opens blob `id` for reading.
    ///
    /// # Errors
    /// The blob is not there.
    pub(crate) fn open(&self, id: &str) -> io::Result<F::File> {
        self.fs.open_existing(&self.dir.join(id))
    }

    /// Removes a blob; one already gone is fine.
    pub(crate) fn remove(&self, id: &str) {
        // WHY: a blob that stays behind is unreferenced and is swept at the next unclean start; nothing reads it.
        if self.fs.remove_file(&self.dir.join(id)).is_err() {
            return;
        }
    }

    /// Removes staging files that belong to no running process and, when `live` is given, blobs nothing refers to.
    pub(crate) fn sweep(&self, live: Option<&HashSet<String>>) {
        let Ok(entries) = self.fs.list_dir(&self.dir) else {
            return;
        };
        for e in entries.iter().filter(|e| !e.is_dir) {
            let stale_part = e.name.ends_with(PART) && !e.name.starts_with(&self.run);
            let orphan = !e.name.ends_with(PART) && live.is_some_and(|l| !l.contains(&e.name));
            if stale_part || orphan {
                self.remove(&e.name);
            }
        }
    }

    /// The file system.
    pub(crate) fn fs(&self) -> &F {
        &self.fs
    }
}

impl<F: Fs> Staging<F> {
    /// The staging file, to write from a blocking thread.
    pub(crate) fn file(&self) -> Arc<F::File> {
        Arc::clone(&self.file)
    }
}

#[cfg(test)]
mod tests {
    use doze_fs::sim::{Cut, DataCut, NameCut, SimFs};

    use super::*;

    fn write(s: &mut Staging<SimFs>, data: &[u8]) {
        s.file.write_all_at(data, s.len).unwrap();
        s.len += data.len() as u64;
    }

    fn blobs(fs: &SimFs, run: &str) -> Blobs<SimFs> {
        Blobs::with_run(fs.clone(), Path::new("/data"), run.to_owned())
    }

    #[test]
    fn a_published_blob_survives_a_power_cut_and_a_half_written_one_does_not() {
        let fs = SimFs::default();
        let b = blobs(&fs, "run1");
        let mut done = b.begin().unwrap();
        write(&mut done, b"whole");
        let id = b.publish(&done).unwrap();
        let mut half = b.begin().unwrap();
        write(&mut half, b"partial");
        let after = fs.power_cut(Cut { data: DataCut::Drop, names: NameCut::Durable, seed: 1 });
        let b2 = blobs(&after, "run2");
        let mut buf = [0u8; 5];
        assert_eq!(b2.open(&id).unwrap().read_at(&mut buf, 0).unwrap(), 5);
        assert_eq!(&buf, b"whole");
        b2.sweep(None);
        let names: Vec<String> = after
            .list_dir(Path::new("/data/blobs"))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, [id], "the half-written file is gone");
    }

    #[test]
    fn an_unreferenced_blob_is_swept_when_the_live_set_is_known() {
        let fs = SimFs::default();
        let b = blobs(&fs, "r");
        let mut a = b.begin().unwrap();
        write(&mut a, b"kept");
        let kept = b.publish(&a).unwrap();
        let mut o = b.begin().unwrap();
        write(&mut o, b"lost");
        let orphan = b.publish(&o).unwrap();
        b.sweep(Some(&HashSet::from([kept.clone()])));
        assert!(b.open(&kept).is_ok());
        assert!(b.open(&orphan).is_err());
    }

    #[test]
    fn a_failed_flush_leaves_no_blob() {
        let fs = SimFs::default();
        let b = blobs(&fs, "r");
        let mut s = b.begin().unwrap();
        write(&mut s, b"x");
        fs.fail(doze_fs::sim::Op::Rename, 1, doze_fs::sim::Failure::Io);
        assert!(b.publish(&s).is_err());
        let names = fs.list_dir(Path::new("/data/blobs")).unwrap();
        assert!(names.is_empty(), "{names:?}");
    }
}
