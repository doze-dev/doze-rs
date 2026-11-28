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
    /// Where a batch begins, as an offset and a position (see [`Self::hinted`]): where the last read ended, and where the
    /// last append began.
    hints: Arc<[AtomicU64; 2]>,
}

impl<F: Fs> Clone for SegmentView<F> {
    fn clone(&self) -> Self {
        Self {
            base: self.base,
            log: Arc::clone(&self.log),
            committed: Arc::clone(&self.committed),
            index: self.index.clone(),
            hints: Arc::clone(&self.hints),
        }
    }
}

/// No hint.
const NO_HINT: u64 = u64::MAX;

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
            hints: Arc::new([AtomicU64::new(NO_HINT), AtomicU64::new(NO_HINT)]),
        }
    }

    /// The position of the batch that begins at `offset`, if it is where the last read of this segment ended or where the
    /// last append to it began.
    ///
    /// A consumer that keeps up asks for one or the other: where its own last read stopped, or (caught up and woken by an
    /// append) where that append began. Then no index entry has to be looked up (a binary search over the index file, one
    /// small read per step). Each hint packs the offset (from the base, as the index keeps it) and the position in one word,
    /// so a reader never sees one from one update and the other from the next. A segment's bytes up to what is committed
    /// never change, so a hint stays true; the caller still checks the header there, and looks the offset up if it is not a
    /// batch.
    pub(crate) fn hinted(&self, offset: i64) -> Option<u64> {
        self.hints.iter().find_map(|hint| {
            let packed = hint.load(Ordering::Acquire);
            if packed == NO_HINT {
                return None;
            }
            let (relative, pos) = (packed >> 32, packed & u64::from(u32::MAX));
            let at = self.base.get().checked_add(i64::try_from(relative).ok()?)?;
            (at == offset && pos < self.committed()).then_some(pos)
        })
    }

    /// Notes that the last read ended where a batch begins at `pos` with `offset` (see [`Self::hinted`]).
    pub(crate) fn remember(&self, offset: i64, pos: u64) {
        self.note(0, offset, pos);
    }

    /// Notes that the last append began at `pos`, its first batch at `offset` (see [`Self::hinted`]).
    pub(crate) fn remember_append(&self, offset: i64, pos: u64) {
        self.note(1, offset, pos);
    }

    /// An offset or a position past what one word holds (a segment of 4 GiB or more) is not noted.
    fn note(&self, slot: usize, offset: i64, pos: u64) {
        let relative = offset
            .checked_sub(self.base.get())
            .and_then(|r| u32::try_from(r).ok());
        if let (Some(relative), Ok(pos), Some(hint)) =
            (relative, u32::try_from(pos), self.hints.get(slot))
        {
            let packed = (u64::from(relative) << 32) | u64::from(pos);
            if packed != NO_HINT {
                hint.store(packed, Ordering::Release);
            }
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

    /// Up to `len` committed bytes at `pos`, in a buffer that is handed on as it is (no copy into `Bytes`). A read of 64 KiB
    /// or more goes in a buffer from the pool, which returns to it when the last copy is dropped (once the answer is
    /// written to the socket): a consumer reading a backlog allocated, zeroed and freed up to a megabyte a partition a
    /// fetch, which macOS's allocator then kept.
    pub(crate) fn read(&self, pos: u64, len: usize) -> io::Result<Bytes> {
        let room = self.committed().saturating_sub(pos);
        let size = usize::try_from(room).unwrap_or(usize::MAX).min(len);
        if crate::pool::pooled(size) {
            let mut buf = crate::pool::POOL.take_filled(size);
            let filled = self.read_into(pos, &mut buf)?;
            buf.truncate(filled);
            return Ok(crate::pool::POOL.share(buf));
        }
        let mut buf = vec![0; size];
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
