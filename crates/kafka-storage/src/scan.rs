//! Walking a segment's batches from the start: where the valid log ends, the offset after its last batch, and
//! the index it should have. The log is its own write-ahead log, so recovery is this walk and a cut.
//!
//! It changes nothing. Opening a partition acts on the result (cut, rewrite the index); reading a closed
//! partition's bounds only looks, so the two cannot disagree about where a partition ends.

use std::io;

use kafka_wire::batch::Peek;
use kafka_wire::ids::{Offset, TimestampMs};

use crate::fs::FsFile;
use crate::index::Entry;

/// The bytes of a batch header: enough to find the batch's extent.
const HEADER: usize = 61;
/// How much a scan reads at a time: a read per batch header and another per body would be two system calls a
/// batch, which over a segment is most of the time it takes.
const WINDOW: usize = 1 << 20;

/// What a walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scanned {
    /// Where the valid log ends, in bytes.
    pub valid_end: u64,
    /// The offset after the last good batch (the segment's base if it has none).
    pub next_offset: Offset,
    /// The sparse index this log should have.
    pub index: Vec<Entry>,
    /// Bytes since the last index entry.
    pub since_index: u64,
    /// The newest timestamp of any record.
    pub max_timestamp: Option<TimestampMs>,
}

/// Serves the many small sequential reads of a walk from one larger read.
struct Window<'a, L: FsFile> {
    file: &'a L,
    size: u64,
    buf: Vec<u8>,
    at: u64,
}

impl<'a, L: FsFile> Window<'a, L> {
    fn new(file: &'a L, size: u64) -> Self {
        Self {
            file,
            size,
            buf: Vec::new(),
            at: 0,
        }
    }

    /// Up to `n` bytes at `pos`, fewer at the end of the file.
    fn read(&mut self, pos: u64, n: usize) -> io::Result<&[u8]> {
        let end = pos + n as u64;
        let held = self.at..self.at + self.buf.len() as u64;
        if !(held.contains(&pos) && end <= held.end) {
            let want = n
                .max(WINDOW)
                .min(usize::try_from(self.size.saturating_sub(pos)).unwrap_or(usize::MAX));
            self.buf.resize(want, 0);
            let mut filled = 0;
            while filled < want {
                let got = self.file.read_at(
                    self.buf.get_mut(filled..).unwrap_or_default(),
                    pos + filled as u64,
                )?;
                if got == 0 {
                    break;
                }
                filled += got;
            }
            self.buf.truncate(filled);
            self.at = pos;
        }
        let from = usize::try_from(pos - self.at).unwrap_or(usize::MAX);
        let slice = self.buf.get(from..).unwrap_or_default();
        Ok(slice.get(..n).unwrap_or(slice))
    }
}

/// Walks the log of `size` bytes whose first offset is `base`. A batch is good when its header is whole, it is
/// format v2, it fits in the file, it holds at least one record, its offsets do not go backwards, and its shape
/// is consistent; its checksum is verified too if it starts at or after `crc_from`. The walk stops at the first
/// batch that is not good: that covers an interrupted append, a zero-filled tail after a power cut, and a body
/// that never fully reached the disk.
pub(crate) fn scan<L: FsFile>(
    log: &L,
    size: u64,
    base: Offset,
    interval: u64,
    crc_from: u64,
) -> io::Result<Scanned> {
    let mut window = Window::new(log, size);
    let mut found = Scanned {
        valid_end: 0,
        next_offset: base,
        index: Vec::new(),
        since_index: 0,
        max_timestamp: None,
    };
    while found.valid_end < size {
        let pos = found.valid_end;
        let Some(peek) = next_good(&mut window, pos, size, found.next_offset, crc_from)? else {
            break;
        };
        found.since_index += peek.total_len;
        if found.index.is_empty() || found.since_index >= interval {
            found.index.push(Entry {
                offset: peek.base_offset,
                position: pos,
            });
            found.since_index = 0;
        }
        found.next_offset = Offset::new(peek.base_offset)
            .and_then(|o| o.checked_add(peek.record_count))
            .unwrap_or(found.next_offset);
        found.max_timestamp = found
            .max_timestamp
            .max(Some(peek.max_timestamp).filter(|t| t.get() >= 0));
        found.valid_end = pos + peek.total_len;
    }
    Ok(found)
}

/// The batch at `pos` if it is good, `None` if the log ends validly before it.
fn next_good<L: FsFile>(
    window: &mut Window<'_, L>,
    pos: u64,
    size: u64,
    expect_at_least: Offset,
    crc_from: u64,
) -> io::Result<Option<Peek>> {
    let Ok(peek) = Peek::read(window.read(pos, HEADER)?) else {
        return Ok(None);
    };
    let fits = pos
        .checked_add(peek.total_len)
        .is_some_and(|end| end <= size);
    let forwards = peek.base_offset >= expect_at_least.get();
    let advances = Offset::new(peek.base_offset).and_then(|o| o.checked_add(peek.record_count));
    if !(fits && forwards && advances.is_some()) {
        return Ok(None);
    }
    if pos >= crc_from {
        let whole = window.read(pos, usize::try_from(peek.total_len).unwrap_or(usize::MAX))?;
        if Peek::read_verified(whole).is_err() {
            return Ok(None);
        }
    }
    Ok(Some(peek))
}

/// The offset after the last batch of a log, found by walking forward from the last index entry (no need to read
/// the whole segment) and judging batches by their headers alone.
pub(crate) fn tail_end<L: FsFile>(
    log: &L,
    size: u64,
    last_entry: Option<Entry>,
    base: Offset,
) -> io::Result<Offset> {
    let (mut pos, mut next) = match last_entry {
        Some(e) => (e.position, Offset::new(e.offset).unwrap_or(base)),
        None => (0, base),
    };
    let mut window = Window::new(log, size);
    while pos < size {
        let Ok(peek) = Peek::read(window.read(pos, HEADER)?) else {
            break;
        };
        let Some(after) =
            Offset::new(peek.base_offset).and_then(|o| o.checked_add(peek.record_count))
        else {
            break;
        };
        if pos.saturating_add(peek.total_len) > size {
            break;
        }
        next = after;
        pos += peek.total_len;
    }
    Ok(next)
}

/// The newest timestamp of any record in a log, read from batch headers alone.
pub(crate) fn max_timestamp<L: FsFile>(log: &L, size: u64) -> io::Result<Option<TimestampMs>> {
    let scanned = scan(log, size, Offset::ZERO, u64::MAX, u64::MAX)?;
    Ok(scanned.max_timestamp)
}

#[cfg(test)]
mod tests;
