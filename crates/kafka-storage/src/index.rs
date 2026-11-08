//! The sparse index: for a segment, a few (offset, byte position) pairs, so a fetch can seek near an offset
//! without scanning the file.
//!
//! The index is derived from the log and the log wins: recovery rebuilds it when they disagree. It is never
//! held in memory. Entries are 16 bytes, written in order, so a lookup is a binary search of preads on the
//! file, which the page cache serves: a sealed segment costs the heap nothing, however large.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::fs::{Fs, FsFile};

pub(crate) const ENTRY_BYTES: u64 = 16;

/// One index entry: the offset of a batch and where it starts in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    pub offset: i64,
    pub position: u64,
}

impl Entry {
    fn encode(self) -> [u8; 16] {
        let mut out = [0; 16];
        let (a, b) = out.split_at_mut(8);
        a.copy_from_slice(&self.offset.to_be_bytes());
        b.copy_from_slice(&self.position.to_be_bytes());
        out
    }

    fn decode(raw: &[u8; 16]) -> Self {
        let (a, b) = raw.split_at(8);
        Self {
            offset: i64::from_be_bytes(a.try_into().unwrap_or_default()),
            position: u64::from_be_bytes(b.try_into().unwrap_or_default()),
        }
    }
}

/// A read-only view of an index file. Clones share the file and the count of entries made visible.
pub(crate) struct IndexView<F: Fs> {
    file: Arc<F::File>,
    visible: Arc<AtomicU64>,
}

impl<F: Fs> Clone for IndexView<F> {
    fn clone(&self) -> Self {
        Self {
            file: Arc::clone(&self.file),
            visible: Arc::clone(&self.visible),
        }
    }
}

impl<F: Fs> IndexView<F> {
    /// How many entries a reader may use.
    pub(crate) fn len(&self) -> u64 {
        self.visible.load(Ordering::Acquire)
    }

    fn get(&self, i: u64) -> io::Result<Entry> {
        let mut raw = [0u8; 16];
        let got = self.file.read_at(&mut raw, i * ENTRY_BYTES)?;
        if got < raw.len() {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        Ok(Entry::decode(&raw))
    }

    /// The byte position of the largest entry whose offset is at or below `target`, or 0 (scan from the start)
    /// if there is none.
    pub(crate) fn lookup(&self, target: i64) -> io::Result<u64> {
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.get(mid)?.offset > target {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        match lo.checked_sub(1) {
            Some(i) => Ok(self.get(i)?.position),
            None => Ok(0),
        }
    }

    /// Every visible entry.
    pub(crate) fn entries(&self) -> io::Result<Vec<Entry>> {
        (0..self.len()).map(|i| self.get(i)).collect()
    }
}

/// The index of the active segment: the view, and what only the writer may do.
pub(crate) struct IndexWriter<F: Fs> {
    view: IndexView<F>,
    /// Entries written, some perhaps not yet visible to readers.
    written: u64,
    dirty: bool,
}

impl<F: Fs> IndexWriter<F> {
    /// Opens an index file; a trailing partial entry is ignored (and overwritten by the next one).
    pub(crate) fn new(file: F::File) -> io::Result<Self> {
        let count = file.size()? / ENTRY_BYTES;
        Ok(Self {
            view: IndexView {
                file: Arc::new(file),
                visible: Arc::new(AtomicU64::new(count)),
            },
            written: count,
            dirty: false,
        })
    }

    pub(crate) fn view(&self) -> IndexView<F> {
        self.view.clone()
    }

    pub(crate) fn len(&self) -> u64 {
        self.written
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes an entry. It becomes visible to readers at [`IndexWriter::publish`].
    pub(crate) fn append(&mut self, entry: Entry) -> io::Result<()> {
        // Dirty before the write: a write that fails part-way may still have changed the file.
        self.dirty = true;
        self.view
            .file
            .write_all_at(&entry.encode(), self.written * ENTRY_BYTES)?;
        self.written += 1;
        Ok(())
    }

    pub(crate) fn publish(&self) {
        self.view.visible.store(self.written, Ordering::Release);
    }

    /// Drops every entry after the first `keep`. A cut that fails leaves bytes past the count; the next
    /// append writes over them.
    pub(crate) fn truncate(&mut self, keep: u64) {
        if self.written > keep {
            self.written = keep;
            self.dirty = true;
            let _ = self.view.file.set_len(keep * ENTRY_BYTES);
        }
    }

    /// Replaces every entry (recovery: the log wins).
    pub(crate) fn rewrite(&mut self, entries: &[Entry]) -> io::Result<()> {
        self.dirty = true;
        self.view.file.set_len(0)?;
        let raw: Vec<u8> = entries.iter().flat_map(|e| e.encode()).collect();
        if !raw.is_empty() {
            self.view.file.write_all_at(&raw, 0)?;
        }
        self.written = entries.len() as u64;
        self.publish();
        Ok(())
    }

    /// Flushes the index if it changed.
    pub(crate) fn sync(&mut self) -> io::Result<()> {
        if self.dirty {
            self.view.file.sync_data()?;
            self.dirty = false;
        }
        Ok(())
    }

    /// The read side, for a segment being sealed to keep.
    pub(crate) fn into_view(self) -> IndexView<F> {
        self.view
    }
}
