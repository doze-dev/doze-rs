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

    #[cfg(test)]
    pub(crate) fn index(&self) -> &IndexView<F> {
        &self.index
    }

    /// Up to `len` committed bytes at `pos`; fewer at the end of what is committed, none past it.
    pub(crate) fn read(&self, pos: u64, len: usize) -> io::Result<Bytes> {
        let room = self.committed().saturating_sub(pos);
        let want = usize::try_from(room).unwrap_or(usize::MAX).min(len);
        let mut buf = vec![0; want];
        let mut filled = 0;
        while filled < want {
            let got = self.log.read_at(
                buf.get_mut(filled..).unwrap_or_default(),
                pos + filled as u64,
            )?;
            if got == 0 {
                break;
            }
            filled += got;
        }
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
