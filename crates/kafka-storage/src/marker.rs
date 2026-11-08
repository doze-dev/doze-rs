//! The clean-stop marker: what a stop carries to the next start that the partitions cannot say themselves.
//!
//! It is written, durably, as the last act of a stop that flushed and closed every partition it had open, and
//! removed, durably, as the first act of the next start. Finding it means the previous process flushed what it
//! wrote, so what is read back from a partition is on disk and needs no flush before it is acknowledged again.
//! Not finding it means the previous process was killed, lost power, or failed a flush on its way out: every
//! partition then starts not vouched for. Apache Kafka keeps the same marker for the same decision.
//!
//! A stop can vouch only for what it flushed. Partitions open lazily, so after an unclean stop a run may never
//! touch some of them, and its own clean stop says nothing about those: the marker lists them, and they stay
//! not vouched for until a run that opens them stops cleanly.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{Doing, StorageError};
use crate::fs::{Fs, read_file, write_durably};
use crate::log::Trust;

const FILE: &str = "clean-shutdown";
const HEADER: &str = "doze-kafka clean shutdown 1";

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// What the last stop left, read at start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopState {
    /// The partitions the last stop did not cover, if it was clean; `None` if it was not.
    not_covered: Option<BTreeSet<String>>,
}

impl StopState {
    /// Reads the marker and removes it durably, before anything can be written: a marker that came back after
    /// a power cut would vouch for a log this run was in the middle of writing.
    ///
    /// # Errors
    /// A failed read, removal or flush.
    pub fn begin<F: Fs>(fs: &F, data_dir: &Path) -> Result<Self, StorageError> {
        let raw = read_file(fs, &path(data_dir)).doing("reading the clean-stop marker")?;
        let state = Self {
            not_covered: raw.as_deref().and_then(parse),
        };
        fs.remove_file(&path(data_dir))
            .doing("removing the clean-stop marker")?;
        fs.sync_dir(data_dir).doing("flushing the data directory")?;
        Ok(state)
    }

    /// Whether the last stop was clean.
    #[must_use]
    pub fn was_clean(&self) -> bool {
        self.not_covered.is_some()
    }

    /// Whether the last stop vouches for the partition directory called `name`.
    #[must_use]
    pub fn trust(&self, name: &str) -> Trust {
        match &self.not_covered {
            Some(not) if !not.contains(name) => Trust::Vouched,
            _ => Trust::Unvouched,
        }
    }

    /// The partitions the last stop did not cover.
    pub fn not_covered(&self) -> impl Iterator<Item = &str> {
        self.not_covered.iter().flatten().map(String::as_str)
    }
}

/// A file of a header line and then one name per line. A torn last line is not a name, so a marker that reached
/// the disk before all its bytes did reads as not there.
fn parse(raw: &[u8]) -> Option<BTreeSet<String>> {
    let text = std::str::from_utf8(raw).ok()?.strip_suffix('\n')?;
    let mut lines = text.split('\n');
    (lines.next()? == HEADER).then(|| lines.filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

/// Records that this run stopped cleanly, flushing everything it had open, and which partitions that does not
/// cover.
///
/// # Errors
/// A failed write or flush.
pub fn record_clean_stop<F: Fs>(
    fs: &F,
    data_dir: &Path,
    not_covered: &[String],
) -> Result<(), StorageError> {
    let mut text = format!("{HEADER}\n");
    for name in not_covered {
        text.push_str(name);
        text.push('\n');
    }
    write_durably(fs, &path(data_dir), text.as_bytes()).doing("writing the clean-stop marker")
}

#[cfg(test)]
mod tests;
