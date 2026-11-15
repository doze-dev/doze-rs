//! One partition's read for a ShareFetch: whole batches from the share-partition's next fetch offset (to the last stable
//! offset under `read_committed`), what each batch is, and the slice of them a response carries.

use bytes::Bytes;
use kafka_storage::StorageError;
use kafka_storage::fs::Fs;
use kafka_wire::batch::{Marker, Peek};
use kafka_wire::ids::Offset;

use crate::partitions::Partition;
use crate::share::partition::{Aborted, AcquiredRange, Span};

/// What a read found.
pub(super) enum Read {
    /// Batches (possibly none), each with its size, and the aborted transactions they touch (`read_committed`).
    Data {
        bytes: Bytes,
        spans: Vec<(Span, usize)>,
        aborted: Vec<Aborted>,
    },
    /// The offset is before the log's start (records were deleted): it starts at `log_start` now.
    Gone { log_start: i64 },
    Failed,
}

pub(super) fn read<F: Fs>(p: &Partition<F>, from: i64, max_bytes: usize, read_committed: bool) -> Read {
    let reader = p.reader();
    let (cap, aborted) = if read_committed {
        let (stable, aborted) = p.committed(from, |a| Aborted {
            producer: a.producer,
            first: a.first,
        });
        (Some(stable), aborted)
    } else {
        (None, Vec::new())
    };
    let Some(at) = Offset::new(from) else {
        return Read::Gone {
            log_start: reader.start_offset().get(),
        };
    };
    match reader.read(at, max_bytes, cap) {
        Ok(bytes) => Read::Data {
            spans: spans_of(&bytes),
            bytes,
            aborted,
        },
        Err(StorageError::OutOfRange { start, .. }) if from < start => Read::Gone { log_start: start },
        Err(StorageError::OutOfRange { .. }) => Read::Data {
            bytes: Bytes::new(),
            spans: Vec::new(),
            aborted,
        },
        Err(e) => {
            tracing::error!(error = %e, "a share fetch could not read a partition");
            Read::Failed
        }
    }
}

/// Each whole batch in `bytes`, with its size.
fn spans_of(bytes: &Bytes) -> Vec<(Span, usize)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some(head) = bytes.get(pos..) {
        let Ok(peek) = Peek::read(head) else {
            break;
        };
        let Ok(len) = usize::try_from(peek.total_len) else {
            break;
        };
        let Some(whole) = bytes.get(pos..pos + len) else {
            break;
        };
        let control = peek.attributes.is_control();
        out.push((
            Span {
                base: peek.base_offset,
                last: peek.base_offset + i64::from(peek.last_offset_delta),
                producer: peek.producer.map(|p| p.id.get()),
                transactional: peek.attributes.is_transactional(),
                abort_marker: control && Marker::read(whole) == Some(Marker::Abort),
            },
            len,
        ));
        pos += len;
    }
    out
}

/// Kafka's `maybeSliceFetchRecords`: from the last batch beginning at or before the first acquired offset, to the last
/// beginning at or before the last acquired one. A single batch, or nothing to cut, is returned whole.
pub(super) fn slice(bytes: &Bytes, spans: &[(Span, usize)], acquired: &[AcquiredRange]) -> Bytes {
    let (Some(first), Some(last)) = (acquired.first(), acquired.last()) else {
        return Bytes::new();
    };
    if spans.len() <= 1 {
        return bytes.clone();
    }
    let mut start = 0usize;
    let mut at = 0usize;
    let mut first_overlap = 0usize;
    for (i, (s, len)) in spans.iter().enumerate() {
        if s.base <= first.first {
            first_overlap = i;
            start = at;
        }
        at += len;
    }
    let mut end = start;
    for (s, len) in spans.iter().skip(first_overlap) {
        if s.base > last.last && end > start {
            break;
        }
        end += len;
    }
    bytes.slice(start..end.min(bytes.len()))
}
