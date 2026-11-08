//! The read side of a segment, shared by readers and the writer.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use kafka_wire::ids::Offset;

use crate::fs::{Fs, FsFile};
use crate::index::IndexView;

/// A cheap, cloneable, read-only handle on a segment: its log file, how much of it is committed, and its
/// index. Reads see only committed bytes, so a reader never meets a half-written append.
pub(crate) struct SegmentView<F: Fs> {
    base: Offset,
    log: Arc<F::File>,
    committed: Arc<AtomicU64>,
    index: IndexView<F>,
}

impl<F: Fs> Clone for SegmentView<F> {
    fn clone(&self) -> Self {
        Self {
            base: self.base,
            log: Arc::clone(&self.log),
            committed: Arc::clone(&self.committed),
            index: self.index.clone(),
        }
    }
}

impl<F: Fs> SegmentView<F> {
    pub(crate) fn new(
        base: Offset,
        log: Arc<F::File>,
        committed: Arc<AtomicU64>,
        index: IndexView<F>,
    ) -> Self {
        Self {
            base,
            log,
            committed,
            index,
        }
    }

    pub(crate) fn base(&self) -> Offset {
        self.base
    }

    /// The bytes of the log that readers may see.
    pub(crate) fn committed(&self) -> u64 {
        self.committed.load(Ordering::Acquire)
    }

    pub(crate) fn publish(&self, len: u64) {
        self.committed.store(len, Ordering::Release);
    }

    pub(crate) fn index(&self) -> &IndexView<F> {
        &self.index
    }

    /// Fills `buf` with committed bytes at `pos` and says how many: fewer at the end of what is committed,
    /// none past it. Nothing is allocated; this is how a header is looked at.
    pub(crate) fn read_into(&self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        let room = usize::try_from(self.committed().saturating_sub(pos)).unwrap_or(usize::MAX);
        let want = buf.len().min(room);
        let mut filled = 0;
        while filled < want {
            let dst = buf.get_mut(filled..want).unwrap_or_default();
            let got = self.log.read_at(dst, pos + filled as u64)?;
            if got == 0 {
                break;
            }
            filled += got;
        }
        Ok(filled)
    }

    /// Up to `len` committed bytes at `pos`, in a buffer that is handed on as it is (no copy into `Bytes`).
    pub(crate) fn read(&self, pos: u64, len: usize) -> io::Result<Bytes> {
        let room = self.committed().saturating_sub(pos);
        let mut buf = vec![0; usize::try_from(room).unwrap_or(usize::MAX).min(len)];
        let filled = self.read_into(pos, &mut buf)?;
        buf.truncate(filled);
        Ok(Bytes::from(buf))
    }

    /// Where to start reading for `offset`: the position of the nearest index entry at or below it.
    pub(crate) fn seek(&self, offset: i64) -> io::Result<u64> {
        self.index.lookup(offset)
    }

    pub(crate) fn log_file(&self) -> &Arc<F::File> {
        &self.log
    }
}
