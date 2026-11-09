//! A sealed segment read batch by batch, with the few large reads a pass over it wants rather than one for each batch.

use std::io;

use bytes::Bytes;
use kafka_wire::batch::Peek;

use crate::fs::Fs;
use crate::segment::SegmentView;

/// How much one read takes (a batch larger than that is read whole).
const WINDOW: usize = 1 << 20;

/// A batch and where it is in the segment.
pub(crate) struct Found {
    pub position: u64,
    pub bytes: Bytes,
    pub peek: Peek,
}

pub(crate) struct Source<'a, F: Fs> {
    view: &'a SegmentView<F>,
    size: u64,
    /// Where `held` begins in the file.
    at: u64,
    held: Bytes,
}

impl<'a, F: Fs> Source<'a, F> {
    /// The segment from byte `from`, which is the start of a batch.
    pub(crate) fn new(view: &'a SegmentView<F>, from: u64) -> Self {
        Self {
            view,
            size: view.committed(),
            at: from,
            held: Bytes::new(),
        }
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    /// The next whole batch, `None` at the end of the segment.
    ///
    /// # Errors
    /// A failed read, or bytes that are not a batch (a sealed segment's bytes all are).
    pub(crate) fn next(&mut self) -> io::Result<Option<Found>> {
        if self.at >= self.size {
            return Ok(None);
        }
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
        let mut need = kafka_wire::batch::HEADER_LEN;
        loop {
            if self.held.len() < need {
                let want = need.max(WINDOW);
                self.held = self.view.read(self.at, want)?;
                if self.held.len() < need {
                    return Err(bad("a segment ends inside a batch"));
                }
            }
            let peek = Peek::read(&self.held).map_err(|e| bad(&e.to_string()))?;
            let total = usize::try_from(peek.total_len).map_err(|_| bad("a batch is too long"))?;
            if self.held.len() >= total {
                let position = self.at;
                let bytes = self.held.split_to(total);
                self.at += peek.total_len;
                return Ok(Some(Found {
                    position,
                    bytes,
                    peek,
                }));
            }
            need = total;
            self.held = Bytes::new();
        }
    }
}
