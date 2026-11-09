//! Reading: whole batches, from the one that holds an offset, straight off the files.

use std::sync::Arc;

use std::io;

use bytes::Bytes;
use kafka_wire::batch::Peek;
use kafka_wire::ids::Offset;
use tokio::sync::watch;

use super::shared::Shared;
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::segment::SegmentView;

/// The bytes of a batch header: what a read needs to step from one batch to the next.
const HEADER: usize = kafka_wire::batch::HEADER_LEN;

/// A reader of one partition's log. Cloning is cheap. Readers see only fully appended payloads and never wait
/// for the writer; the log is never pulled into the heap, so memory does not grow with the data.
pub struct LogReader<F: Fs> {
    pub(super) shared: Arc<Shared<F>>,
}

impl<F: Fs> Clone for LogReader<F> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<F: Fs> std::fmt::Debug for LogReader<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogReader")
            .field("start", &self.start_offset())
            .field("end", &self.end_offset())
            .finish()
    }
}

impl<F: Fs> LogReader<F> {
    pub(super) fn new(shared: Arc<Shared<F>>) -> Self {
        Self { shared }
    }

    /// The oldest offset still held.
    #[must_use]
    pub fn start_offset(&self) -> Offset {
        self.shared.start()
    }

    /// How many segments the log has now. Each holds two files open, so this is what the log costs in descriptors.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.shared.segments.load().len()
    }

    /// The offset the next record will get.
    #[must_use]
    pub fn end_offset(&self) -> Offset {
        self.shared.end()
    }

    /// A channel that changes whenever the end offset does. Wait on it to wait for data: a change made after
    /// the receiver was created is never missed, so reading and then waiting cannot lose a wake-up.
    #[must_use]
    pub fn watch_end(&self) -> watch::Receiver<Offset> {
        self.shared.end.subscribe()
    }

    /// One read, as the files have it: nothing skipped, so that a walk of the log sees every batch.
    pub(super) fn read_once(
        &self,
        from: Offset,
        max_bytes: usize,
        limit: Option<Offset>,
    ) -> Result<Bytes, StorageError> {
        let (start, end) = (self.start_offset(), self.end_offset());
        if from < start || from > end {
            return Err(StorageError::OutOfRange {
                requested: from.get(),
                start: start.get(),
                end: end.get(),
            });
        }
        if from == end {
            return Ok(Bytes::new());
        }
        // The end offset is the one answer to what is in the log. The writer makes a payload's bytes readable a
        // moment before it announces the new end, so a read could otherwise return batches the end has not
        // reached yet, and the reader's next request (for the offset after them) would be past the end it sees.
        // Never return a batch that begins at or after the end observed here.
        // (A limit short of the end also bounds how much of a segment is worth reading.)
        let bounded = limit.is_some_and(|l| l < end);
        let limit = Some(limit.map_or(end, |l| l.min(end)));
        let segments = self.shared.segments.load();
        let first = segments
            .partition_point(|s| s.base() <= from)
            .saturating_sub(1);
        // A segment can have nothing at or after the offset (compaction empties tails), and the records that
        // follow are in the next one: carry on there rather than saying the reader has caught up.
        for (i, segment) in segments.iter().enumerate().skip(first) {
            let at = if i == first { from } else { segment.base() };
            if let Some(found) =
                read_segment(segment, at, max_bytes, (limit, bounded)).doing("reading the log")?
            {
                return self.still_held(from, found);
            }
        }
        Ok(Bytes::new())
    }
}

/// How much more than `max_bytes` a read asks the file for at first: the batches before the offset that it has to
/// step over (the index puts it within one interval and one batch of them), so that one read usually does it all.
const HEADROOM: usize = 16 << 10;

/// How far past the index entry nearest a limit that falls in the segment a read looks: the batch holding the limit
/// begins within an index interval of that entry, and its header is what is wanted. (A read that needs more asks again.)
const LIMIT_SLACK: u64 = 64 << 10;

#[cfg(test)]
thread_local! {
    /// The most a read on this thread has asked the file for in one go.
    pub(super) static BIGGEST_ASK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many answers on this thread were copied out of a larger buffer.
    pub(super) static COPIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A stretch of a segment read in one go. Headers are looked at in it, and the answer is cut out of it, without
/// reading or copying again.
struct Chunk {
    at: u64,
    bytes: Bytes,
}

impl Chunk {
    fn read<F: Fs>(segment: &SegmentView<F>, pos: u64, len: usize) -> io::Result<Self> {
        #[cfg(test)]
        BIGGEST_ASK.with(|b| b.set(b.get().max(len)));
        Ok(Self {
            at: pos,
            bytes: segment.read(pos, len)?,
        })
    }

    /// The part of the chunk from `pos`, if `len` bytes of it are there.
    fn get(&self, pos: u64, len: usize) -> Option<Bytes> {
        let from = usize::try_from(pos.checked_sub(self.at)?).ok()?;
        let to = from.checked_add(len)?;
        (to <= self.bytes.len()).then(|| self.bytes.slice(from..to))
    }
}

fn not_a_batch(pos: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the committed bytes at position {pos} of a segment are not a record batch"),
    )
}

/// The batches of one segment from the first that holds an offset at or after `from`; `None` if it has none.
///
/// The file is read once for the batches asked for (and a little before them), and headers are walked in memory.
/// Bytes that are committed and are not a batch are damage, and an error: never a gap to read past.
fn read_segment<F: Fs>(
    segment: &SegmentView<F>,
    from: Offset,
    max_bytes: usize,
    (limit, bounded): (Option<Offset>, bool),
) -> io::Result<Option<Bytes>> {
    let committed = segment.committed();
    let mut pos = segment.seek(from.get())?;
    // An index entry is the start of a batch, so before the end of the log: one at or past it is a damaged index,
    // and reading on from the next segment would silently skip every record in between.
    if pos > 0 && pos >= committed {
        return Err(not_a_batch(pos));
    }
    // A limit inside this segment (a read-committed fetch's last stable offset, or the end of the log) bounds what is
    // worth reading: nothing at or past it is returned, however much room `max_bytes` leaves.
    let reach = limit
        .filter(|_| bounded)
        .and_then(|l| segment.seek(l.get()).ok())
        .map_or(u64::MAX, |p| p.saturating_add(LIMIT_SLACK));
    let ask = |pos: u64| {
        let wanted = usize::try_from(reach.saturating_sub(pos)).unwrap_or(usize::MAX);
        usize::try_from(committed.saturating_sub(pos)).map_or(usize::MAX, |room| {
            room.min(max_bytes.saturating_add(HEADROOM))
                .min(wanted.max(HEADROOM))
        })
    };
    let mut chunk = Chunk::read(segment, pos, ask(pos))?;
    // Step over the batches that end before the offset.
    let first = loop {
        if pos >= committed {
            return Ok(None);
        }
        let peek = peek_at(segment, &mut chunk, pos, committed, ask(pos))?;
        if peek.next_offset().unwrap_or(i64::MAX) > from.get() {
            break peek;
        }
        pos += peek.total_len;
    };
    // Take whole batches while they fit, but always the first.
    let (start, mut total, mut peek) = (pos, 0u64, first);
    loop {
        if limit.is_some_and(|l| peek.base_offset >= l.get())
            || (total > 0 && total + peek.total_len > max_bytes as u64)
        {
            break;
        }
        total += peek.total_len;
        if start + total >= committed {
            break;
        }
        peek = peek_at(
            segment,
            &mut chunk,
            start + total,
            committed,
            ask(start + total),
        )?;
    }
    let len = usize::try_from(total).unwrap_or(usize::MAX);
    match chunk.get(start, len) {
        // A small answer is not kept as a window on a large buffer: the caller would hold all of it.
        Some(inside) if inside.len().saturating_mul(8) < chunk.bytes.len() => {
            #[cfg(test)]
            COPIED.with(|c| c.set(c.get() + 1));
            Ok(Some(Bytes::copy_from_slice(&inside)))
        }
        Some(inside) => Ok(Some(inside)),
        None => {
            // The first batch is bigger than what was asked for. A file that turns out shorter than the headers
            // promised has been cut under us: that is damage, and not a short answer to hand on as a batch.
            let bytes = segment.read(start, len)?;
            if bytes.len() < len {
                return Err(not_a_batch(start));
            }
            Ok(Some(bytes))
        }
    }
}

/// The header of the whole batch committed at `pos`, looked up in `chunk` (read again from `pos` if it is not
/// there).
fn peek_at<F: Fs>(
    segment: &SegmentView<F>,
    chunk: &mut Chunk,
    pos: u64,
    committed: u64,
    ask: usize,
) -> io::Result<Peek> {
    if chunk.get(pos, HEADER).is_none() {
        *chunk = Chunk::read(segment, pos, ask.max(HEADER))?;
    }
    let header = chunk.get(pos, HEADER).ok_or_else(|| not_a_batch(pos))?;
    let peek = Peek::read(&header).map_err(|_| not_a_batch(pos))?;
    if pos + peek.total_len > committed {
        return Err(not_a_batch(pos));
    }
    Ok(peek)
}

impl<F: Fs> LogReader<F> {
    /// Retention may have moved the start past `from` while the bytes were being read: then they are not
    /// the offset asked for any more, and the reader is told so rather than handed what follows.
    fn still_held(&self, from: Offset, found: Bytes) -> Result<Bytes, StorageError> {
        let start = self.start_offset();
        if from < start {
            return Err(StorageError::OutOfRange {
                requested: from.get(),
                start: start.get(),
                end: self.end_offset().get(),
            });
        }
        Ok(found)
    }
}
