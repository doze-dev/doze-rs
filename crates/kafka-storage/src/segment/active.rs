//! The segment that takes appends.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;

use kafka_wire::batch::Batch;
use kafka_wire::ids::{Offset, TimestampMs};

use super::{SealedSegment, SegmentView, index_path, log_path};
use crate::fs::{Fs, FsFile};
use crate::index::{Entry, IndexWriter};
use crate::scan::Scanned;

/// Where an active segment's appends stood, to take back a payload that failed part-way.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    size: u64,
    since_index: u64,
    entries: u64,
    modified_ms: i64,
    max_timestamp: Option<TimestampMs>,
}

/// The one segment of a partition that is written: its log file, its index, and what the writer knows that
/// readers do not yet (bytes written and not yet published).
pub(crate) struct ActiveSegment<F: Fs> {
    view: SegmentView<F>,
    index: IndexWriter<F>,
    /// Bytes written; readers see [`SegmentView::committed`], which trails this until a payload is published.
    size: u64,
    since_index: u64,
    interval: u64,
    log_dirty: bool,
    modified_ms: i64,
    max_timestamp: Option<TimestampMs>,
}

impl<F: Fs> ActiveSegment<F> {
    /// Opens the segment that begins at `base`, creating its files if they are not there. Says whether it made
    /// either: the names are then only in memory until the directory is flushed.
    pub(crate) fn open(
        fs: &F,
        dir: &Path,
        base: Offset,
        interval: u64,
    ) -> io::Result<(Self, bool)> {
        let (log, made_log) = fs.open_or_create(&log_path(dir, base))?;
        let (index, made_index) = match fs.open_or_create(&index_path(dir, base)) {
            Ok(opened) => opened,
            Err(e) => {
                if made_log {
                    let _ = fs.remove_file(&log_path(dir, base));
                }
                return Err(e);
            }
        };
        let size = log.size()?;
        let modified_ms = log.modified_ms()?;
        let index = IndexWriter::new(index)?;
        let view = SegmentView::new(
            base,
            Arc::new(log),
            Arc::new(AtomicU64::new(size)),
            index.view(),
        );
        let segment = Self {
            view,
            index,
            size,
            since_index: 0,
            interval,
            log_dirty: false,
            modified_ms,
            max_timestamp: None,
        };
        Ok((segment, made_log || made_index))
    }

    pub(crate) fn view(&self) -> SegmentView<F> {
        self.view.clone()
    }

    pub(crate) fn base(&self) -> Offset {
        self.view.base()
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.log_dirty || self.index.is_dirty()
    }

    /// Marks the segment as holding bytes nobody knows to be on disk (read back at open without a vouching
    /// stop), so the first sync flushes them.
    pub(crate) fn mark_unflushed(&mut self) {
        self.log_dirty = true;
    }

    pub(crate) fn mark(&self) -> Mark {
        Mark {
            size: self.size,
            since_index: self.since_index,
            entries: self.index.len(),
            modified_ms: self.modified_ms,
            max_timestamp: self.max_timestamp,
        }
    }

    /// Writes a batch at the end of the log and, on crossing the index interval (and for a segment's first
    /// batch), an index entry. The batch carries the offset it was stamped with. Nothing is visible to readers
    /// until [`ActiveSegment::publish`]. A failure leaves no trace: the bytes are cut off and the counts
    /// restored.
    ///
    /// # Errors
    /// A failed write, or a batch that has not been given an offset.
    pub(crate) fn append(&mut self, batch: &Batch, now_ms: i64) -> io::Result<()> {
        let offset = batch.base_offset().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the batch has no assigned offset",
            )
        })?;
        let (pos, bytes) = (self.size, batch.as_bytes());
        let len = bytes.len() as u64;
        let before = self.mark();
        // Dirty before the write: a write that fails part-way has still changed the file, and the cut that
        // undoes it has to reach the disk.
        self.log_dirty = true;
        if let Err(e) = self.view.log_file().write_all_at(bytes, pos) {
            self.cut_to(pos);
            return Err(e);
        }
        self.size += len;
        self.since_index += len;
        self.modified_ms = now_ms;
        self.max_timestamp = self.max_timestamp.max(known(batch.max_timestamp()));
        if self.index.len() == 0 || self.since_index >= self.interval {
            let entry = Entry {
                offset: offset.get(),
                position: pos,
            };
            if let Err(e) = self.index.append(entry) {
                // Bytes left counted would make the next append write the same offset again after them.
                self.rewind(before);
                return Err(e);
            }
            self.since_index = 0;
        }
        Ok(())
    }

    /// Takes back every append since `mark`.
    pub(crate) fn rewind(&mut self, mark: Mark) {
        self.size = mark.size;
        self.since_index = mark.since_index;
        self.modified_ms = mark.modified_ms;
        self.max_timestamp = mark.max_timestamp;
        self.index.truncate(mark.entries);
        self.cut_to(mark.size);
    }

    fn cut_to(&mut self, size: u64) {
        // A cut that fails leaves bytes past the size the segment counts; the next append writes over them,
        // and recovery after a crash before then finds them as it would a torn tail.
        self.log_dirty = true;
        let _ = self.view.log_file().set_len(size);
    }

    /// Makes everything appended so far visible to readers.
    pub(crate) fn publish(&self) {
        self.view.publish(self.size);
        self.index.publish();
    }

    /// Flushes the log and the index, whichever changed.
    pub(crate) fn sync(&mut self) -> io::Result<()> {
        if self.log_dirty {
            self.view.log_file().sync_data()?;
            self.log_dirty = false;
        }
        self.index.sync()
    }

    /// Brings the segment in line with what recovery found: cut the log to its valid length, flush the cut, and
    /// make the index what the log says it should be.
    pub(crate) fn adopt(&mut self, scanned: &Scanned) -> io::Result<()> {
        if scanned.valid_end < self.size {
            let log = self.view.log_file();
            log.set_len(scanned.valid_end)?;
            // The cut is flushed: otherwise a second crash brings back the bytes just decided against.
            log.sync_data()?;
            self.size = scanned.valid_end;
        }
        if self.index.view().entries()? != scanned.index {
            self.index.rewrite(&scanned.index)?;
        }
        self.since_index = scanned.since_index;
        self.max_timestamp = scanned.max_timestamp;
        self.publish();
        Ok(())
    }

    /// The segment, no longer written: from here it is immutable. The caller has flushed it.
    pub(crate) fn into_sealed(self) -> SealedSegment<F> {
        let max_timestamp = OnceLock::new();
        let _ = max_timestamp.set(self.max_timestamp);
        SealedSegment::from_parts(self.view, self.size, self.modified_ms, max_timestamp)
    }
}

/// A timestamp that is one: -1 means a batch has none.
fn known(t: TimestampMs) -> Option<TimestampMs> {
    (t.get() >= 0).then_some(t)
}
