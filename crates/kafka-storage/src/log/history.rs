//! What a partition's recent log says of its producers: read back after a restart, so that an idempotent producer's
//! retry of a batch whose acknowledgement was lost in the stop is recognised and not written twice.

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
        let (start, end) = (self.start_offset(), self.end_offset());
        let mut at = from.map_or(start, |f| f.max(start));
        let mut out = Vec::new();
        while at < end {
            let chunk = self.read(at, STEP, None)?;
            let Some(next) = walk(&chunk, &mut out)? else {
                break;
            };
            if next <= at {
                break;
            }
            at = next;
        }
        Ok(out)
    }
}

/// Collects the producers' batches in `chunk` (whole batches), and says the offset after the last one.
fn walk(chunk: &[u8], out: &mut Vec<Peek>) -> Result<Option<Offset>, StorageError> {
    let mut rest = chunk;
    let mut next = None;
    while !rest.is_empty() {
        let peek = Peek::read(rest).map_err(|e| StorageError::Io {
            doing: "reading the log's recent history",
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;
        let after = peek
            .base_offset
            .saturating_add(i64::from(peek.record_count));
        next = Offset::new(after);
        let Some((_, tail)) = usize::try_from(peek.total_len)
            .ok()
            .and_then(|n| rest.split_at_checked(n))
        else {
            break;
        };
        if peek.producer.is_some() {
            out.push(peek);
        }
        rest = tail;
    }
    Ok(next)
}
