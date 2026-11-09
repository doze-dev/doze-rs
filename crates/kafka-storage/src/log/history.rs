//! What the log's batches say of their producers and transactions, read back after a restart: so that an idempotent
//! producer's retry of a batch whose acknowledgement was lost in the stop is recognised and not written twice, and so
//! that a partition knows which transactions are open and which were aborted.

use kafka_wire::batch::Peek;
use kafka_wire::ids::Offset;

use super::read::LogReader;
use crate::error::StorageError;
use crate::fs::Fs;

/// How much of the log is read in one go while walking it.
const STEP: usize = 1 << 20;

impl<F: Fs> LogReader<F> {
    /// The headers of the batches written by a producer, oldest first, in the last whole segments that hold at least
    /// `bytes` (all of the log if it is smaller). A producer retries within its delivery timeout, which is minutes, so
    /// what matters is what was written just before the stop.
    ///
    /// # Errors
    /// A failed read, or a batch that cannot be read.
    pub fn producer_history(&self, bytes: u64) -> Result<Vec<Peek>, StorageError> {
        let segments = self.shared.segments.load();
        let mut held = 0_u64;
        let mut from = None;
        for segment in segments.iter().rev() {
            from = Some(segment.base());
            held = held.saturating_add(segment.committed());
            if held >= bytes {
                break;
            }
        }
        let mut out = Vec::new();
        let start = self.start_offset();
        self.visit_batches(from.map_or(start, |f| f.max(start)), |peek, _| {
            if peek.producer.is_some() {
                out.push(*peek);
            }
        })?;
        Ok(out)
    }

    /// Calls `visit` with the header and the bytes of every batch from the one that holds `from` to the end of the log,
    /// in order. Batches written while this runs may or may not be visited.
    ///
    /// # Errors
    /// A failed read, or a batch that cannot be read.
    pub fn visit_batches(
        &self,
        from: Offset,
        mut visit: impl FnMut(&Peek, &[u8]),
    ) -> Result<(), StorageError> {
        let end = self.end_offset();
        let mut at = from.max(self.start_offset());
        while at < end {
            let chunk = self.read(at, STEP, None)?;
            let Some(next) = walk(&chunk, &mut visit)? else {
                break;
            };
            if next <= at {
                break;
            }
            at = next;
        }
        Ok(())
    }
}

/// Visits the batches in `chunk` (whole batches), and says the offset after the last one.
fn walk(
    chunk: &[u8],
    visit: &mut impl FnMut(&Peek, &[u8]),
) -> Result<Option<Offset>, StorageError> {
    let mut rest = chunk;
    let mut next = None;
    while !rest.is_empty() {
        let peek = Peek::read(rest).map_err(|e| StorageError::Io {
            doing: "reading the log's batches",
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;
        let after = peek
            .base_offset
            .saturating_add(i64::from(peek.record_count));
        next = Offset::new(after);
        let Some((batch, tail)) = usize::try_from(peek.total_len)
            .ok()
            .and_then(|n| rest.split_at_checked(n))
        else {
            break;
        };
        visit(&peek, batch);
        rest = tail;
    }
    Ok(next)
}
