//! A segment that has a successor: immutable.

use std::io;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, OnceLock};

use kafka_wire::ids::{Offset, TimestampMs};

use super::{SegmentView, index_path, log_path};
use crate::fs::{Fs, FsFile};
use crate::index::IndexWriter;
use crate::scan;

pub(crate) struct SealedSegment<F: Fs> {
    view: SegmentView<F>,
    size: u64,
    modified_ms: i64,
    /// The newest timestamp of any record, found on first use for a segment read back from disk.
    max_timestamp: OnceLock<Option<TimestampMs>>,
}

impl<F: Fs> SealedSegment<F> {
    pub(super) fn from_parts(
        view: SegmentView<F>,
        size: u64,
        modified_ms: i64,
        max_timestamp: OnceLock<Option<TimestampMs>>,
    ) -> Self {
        Self {
            view,
            size,
            modified_ms,
            max_timestamp,
        }
    }

    /// Opens a segment that already has a successor. A sealed segment was flushed whole before its successor
    /// existed, so it is trusted; only an index that is missing altogether is rebuilt, from the headers of its
    /// log (the index is derived data and the log wins).
    pub(crate) fn open(fs: &F, dir: &Path, base: Offset, interval: u64) -> io::Result<Self> {
        let log = fs.open_existing(&log_path(dir, base))?;
        let (index, _) = fs.open_or_create(&index_path(dir, base))?;
        let (size, modified_ms) = (log.size()?, log.modified_ms()?);
        let mut index = IndexWriter::<F>::new(index)?;
        if size > 0 && index.len() == 0 {
            index.rewrite(&scan::scan(&log, size, base, interval, u64::MAX)?.index)?;
        }
        let index = index.into_view();
        let view = SegmentView::new(base, Arc::new(log), Arc::new(AtomicU64::new(size)), index);
        Ok(Self::from_parts(view, size, modified_ms, OnceLock::new()))
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

    /// When the segment was last written.
    pub(crate) fn modified_ms(&self) -> i64 {
        self.modified_ms
    }

    /// The newest timestamp of any record in the segment, if any has one.
    pub(crate) fn max_timestamp(&self) -> io::Result<Option<TimestampMs>> {
        if let Some(known) = self.max_timestamp.get() {
            return Ok(*known);
        }
        let found = scan::max_timestamp(self.view.log_file().as_ref(), self.size)?;
        Ok(*self.max_timestamp.get_or_init(|| found))
    }
}
