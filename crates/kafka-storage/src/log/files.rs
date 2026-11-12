//! What a partition's files are, as the console shows them: each segment's base offset, size and last write, and
//! whether a failed flush has taken the partition offline. Read-only: plain owned values, nothing kept, no writer in
//! reach.

use std::path::Path;

use kafka_wire::ids::Offset;

use super::LogReader;
use crate::error::{Doing, StorageError};
use crate::fs::{Fs, FsFile};
use crate::segment::base_of_log;

/// One segment of a partition, as its files stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentInfo {
    /// The offset of its first record (the name of its files).
    pub base: Offset,
    /// The bytes of its log file that readers may see.
    pub bytes: u64,
    /// When its log file was last written, in milliseconds since the epoch, where the system says.
    pub modified_ms: Option<i64>,
}

impl<F: Fs> LogReader<F> {
    /// The segments of the log now, oldest first. A copy of what the reader's snapshot holds; the log is not touched.
    #[must_use]
    pub fn segments(&self) -> Vec<SegmentInfo> {
        self.shared
            .segments
            .load()
            .iter()
            .map(|s| SegmentInfo {
                base: s.base(),
                bytes: s.committed(),
                modified_ms: s.log_file().modified_ms().ok(),
            })
            .collect()
    }

    /// Why the partition is offline: the first failed flush, in the words the writer recorded it. `None` while it is
    /// online.
    #[must_use]
    pub fn offline_cause(&self) -> Option<String> {
        self.shared
            .sync
            .borrow()
            .failed
            .as_ref()
            .map(ToString::to_string)
    }
}

/// The segments of a partition that is not open, read from its directory: the same facts as
/// [`LogReader::segments`], for a log nobody has open. An absent directory has none.
///
/// # Errors
/// The directory cannot be listed or a segment file cannot be looked at.
pub fn segments_on_disk<F: Fs>(fs: &F, dir: &Path) -> Result<Vec<SegmentInfo>, StorageError> {
    let entries = match fs.list_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(StorageError::Io {
                doing: "listing a partition directory",
                source: e,
            });
        }
    };
    let mut out = Vec::new();
    for entry in entries.iter().filter(|e| !e.is_dir) {
        let Some(base) = base_of_log(&entry.name) else {
            continue;
        };
        let file = fs
            .open_existing(&dir.join(&entry.name))
            .doing("looking at a segment")?;
        out.push(SegmentInfo {
            base,
            bytes: file.size().doing("looking at a segment")?,
            modified_ms: file.modified_ms().ok(),
        });
    }
    out.sort_by_key(|s| s.base);
    Ok(out)
}

#[cfg(test)]
mod tests;
