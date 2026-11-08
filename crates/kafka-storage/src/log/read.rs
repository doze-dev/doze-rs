//! Reading: whole batches, from the one that holds an offset, straight off the files.

use std::sync::Arc;

use bytes::Bytes;
use kafka_wire::batch::Peek;
use kafka_wire::ids::Offset;
use tokio::sync::watch;

use super::shared::Shared;
use crate::error::{Doing, StorageError};
use crate::fs::Fs;
use crate::segment::SegmentView;

/// The bytes of a batch header: what a read needs to step from one batch to the next.
const HEADER: usize = 61;

/// A reader of one partition's log. Cloning is cheap. Readers see only fully appended payloads and never wait
/// for the writer; the log is never pulled into the heap, so memory does not grow with the data.
pub struct LogReader<F: Fs> {
    shared: Arc<Shared<F>>,
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

    /// Whole batches starting at the one that holds `from`, up to `max_bytes` (but always the first one, if
    /// there is one, however large). An offset equal to the end is an empty answer (caught up). With a `limit`
    /// (the last stable offset, for a read-committed fetch), no batch that begins at or after it is returned.
    ///
    /// # Errors
    /// [`StorageError::OutOfRange`] below the log start or above the end; a failed read.
    pub fn read(
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
        let segments = self.shared.segments.load();
        let first = segments
            .partition_point(|s| s.base() <= from)
            .saturating_sub(1);
        // A segment can have nothing at or after the offset (compaction empties tails), and the records that
        // follow are in the next one: carry on there rather than saying the reader has caught up.
        for (i, segment) in segments.iter().enumerate().skip(first) {
            let at = if i == first { from } else { segment.base() };
            if let Some(found) =
                read_segment(segment, at, max_bytes, limit).doing("reading the log")?
            {
                return Ok(found);
            }
        }
        Ok(Bytes::new())
    }
}

/// The batches of one segment from the first that holds an offset at or after `from`; `None` if it has none.
fn read_segment<F: Fs>(
    segment: &SegmentView<F>,
    from: Offset,
    max_bytes: usize,
    limit: Option<Offset>,
) -> std::io::Result<Option<Bytes>> {
    let committed = segment.committed();
    let mut pos = segment.seek(from.get())?;
    while pos < committed {
        let Some(peek) = header_at(segment, pos, committed)? else {
            break;
        };
        if peek
            .base_offset
            .saturating_add(i64::from(peek.record_count))
            <= from.get()
        {
            pos += peek.total_len; // this batch ends before the offset
            continue;
        }
        let len = whole_batches(segment, pos, committed, max_bytes, limit)?;
        return Ok(Some(segment.read(pos, len)?));
    }
    Ok(None)
}

/// The header at `pos`, if a whole batch is committed there.
fn header_at<F: Fs>(
    segment: &SegmentView<F>,
    pos: u64,
    committed: u64,
) -> std::io::Result<Option<Peek>> {
    let Ok(peek) = Peek::read(&segment.read(pos, HEADER)?) else {
        return Ok(None);
    };
    Ok((pos + peek.total_len <= committed).then_some(peek))
}

/// How many bytes from `pos` make whole batches within `max_bytes` (at least the first) and below `limit`.
fn whole_batches<F: Fs>(
    segment: &SegmentView<F>,
    pos: u64,
    committed: u64,
    max_bytes: usize,
    limit: Option<Offset>,
) -> std::io::Result<usize> {
    let mut total: u64 = 0;
    while pos + total < committed {
        let Some(peek) = header_at(segment, pos + total, committed)? else {
            break;
        };
        if limit.is_some_and(|l| peek.base_offset >= l.get()) {
            break;
        }
        if total > 0 && total + peek.total_len > max_bytes as u64 {
            break;
        }
        total += peek.total_len;
    }
    Ok(usize::try_from(total).unwrap_or(usize::MAX))
}
