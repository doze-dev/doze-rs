//! The new file for a segment that cleaning changes. It is begun at the first change: what comes before it is copied
//! from the old file, so a segment that cleaning leaves alone is never written.

use std::io;
use std::path::PathBuf;

use crate::fs::{Fs, FsFile};
use crate::segment::SegmentView;

/// How much is held before it is written, and how much is copied at a time.
const CHUNK: usize = 1 << 20;

pub(crate) struct Rewrite<'a, F: Fs> {
    fs: &'a F,
    view: &'a SegmentView<F>,
    path: PathBuf,
    out: Option<F::File>,
    /// Bytes written to the file, and held to be.
    written: u64,
    held: Vec<u8>,
    /// Source bytes before this are dealt with: copied, or replaced.
    dealt_with: u64,
}

impl<'a, F: Fs> Rewrite<'a, F> {
    pub(crate) fn new(fs: &'a F, view: &'a SegmentView<F>, path: PathBuf) -> Self {
        Self {
            fs,
            view,
            path,
            out: None,
            written: 0,
            held: Vec::new(),
            dealt_with: 0,
        }
    }

    /// The batch at `position` (`length` bytes in the old file) is replaced by `with`, or dropped (`None`).
    pub(crate) fn change(
        &mut self,
        position: u64,
        length: u64,
        with: Option<&[u8]>,
    ) -> io::Result<()> {
        if self.out.is_none() {
            let (file, _) = self.fs.open_or_create(&self.path)?;
            file.set_len(0)?;
            self.out = Some(file);
        }
        self.copy_until(position)?;
        if let Some(bytes) = with {
            self.put(bytes)?;
        }
        self.dealt_with = position + length;
        Ok(())
    }

    /// The new file, whole and flushed, and its size; `None` if nothing was changed.
    pub(crate) fn finish(mut self, size: u64) -> io::Result<Option<u64>> {
        if self.out.is_none() {
            return Ok(None);
        }
        self.copy_until(size)?;
        self.flush()?;
        if let Some(file) = &self.out {
            file.sync_data()?;
        }
        Ok(Some(self.written))
    }

    /// Takes back a file begun and not wanted.
    pub(crate) fn abandon(self) {
        if self.out.is_some() {
            let _ = self.fs.remove_file(&self.path);
        }
    }

    fn copy_until(&mut self, end: u64) -> io::Result<()> {
        while self.dealt_with < end {
            let want = usize::try_from(end - self.dealt_with).map_or(CHUNK, |n| n.min(CHUNK));
            let chunk = self.view.read(self.dealt_with, want)?;
            if chunk.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "a segment is shorter than it was",
                ));
            }
            self.dealt_with += chunk.len() as u64;
            self.put(&chunk)?;
        }
        Ok(())
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.held.extend_from_slice(bytes);
        if self.held.len() >= CHUNK {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let (Some(file), false) = (&self.out, self.held.is_empty()) {
            file.write_all_at(&self.held, self.written)?;
            self.written += self.held.len() as u64;
            self.held.clear();
        }
        Ok(())
    }
}
