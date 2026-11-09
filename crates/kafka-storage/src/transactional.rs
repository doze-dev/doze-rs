//! The note that a partition's log holds, or is about to hold, transactional batches or transaction markers.
//!
//! A partition's open and aborted transactions are read from its batches, and reading them all is as long as the log. A
//! log that never held a transactional batch has none, and must not be read to find that out: so the first one is
//! preceded by this note, a file beside the segments, durable before the batch is written. No note, no transactions.

use std::io;
use std::path::{Path, PathBuf};

use crate::fs::{Fs, write_durably};

const FILE: &str = "transactional";

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Whether the note is there.
pub(crate) fn exists<F: Fs>(fs: &F, dir: &Path) -> bool {
    fs.open_existing(&path(dir)).is_ok()
}

/// Makes the note, durably: a power cut after this returns does not take it away.
pub(crate) fn record<F: Fs>(fs: &F, dir: &Path) -> io::Result<()> {
    write_durably(fs, &path(dir), b"yes\n")
}
