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

    /// When the oldest data on disk dates from: the newest timestamp of the first batch of the first segment (what is below
    /// the log start in that segment uses the disk too). `None` for an empty log, or a batch with no timestamp. One read of a
    /// batch header; nothing kept.
    #[must_use]
    pub fn oldest_ms(&self) -> Option<i64> {
        let segments = self.shared.segments.load();
        let first = segments.first()?;
        first_batch_ms(first.log_file().as_ref(), first.committed())
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

/// [`LogReader::oldest_ms`] for a partition that is not open, from its first segment's file. `None` when there is none, it
/// is empty or unreadable, or its first batch has no timestamp.
#[must_use]
pub fn oldest_on_disk<F: Fs>(fs: &F, dir: &Path) -> Option<i64> {
    let first = fs
        .list_dir(dir)
        .ok()?
        .iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| base_of_log(&e.name))
        .min()?;
    let file = fs
        .open_existing(&crate::segment::log_path(dir, first))
        .ok()?;
    let size = file.size().ok()?;
    first_batch_ms(&file, size)
}

/// The newest timestamp of the batch at the head of a segment of `size` bytes, if it has one.
fn first_batch_ms<L: FsFile + ?Sized>(log: &L, size: u64) -> Option<i64> {
    let mut head = [0u8; kafka_wire::batch::HEADER_LEN];
    if size < head.len() as u64 || log.read_at(&mut head, 0).ok()? < head.len() {
        return None;
    }
    let ms = kafka_wire::batch::Peek::read(&head)
        .ok()?
        .max_timestamp
        .get();
    (ms >= 0).then_some(ms)
}

#[cfg(test)]
mod tests;
