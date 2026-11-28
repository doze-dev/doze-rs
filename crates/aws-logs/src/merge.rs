//! The events of several streams read as one, in the order `FilterLogEvents` answers them (time, then stream name, then the order
//! they arrived), with a bound on the rows one call reads.
//!
//! AWS's `FilterLogEvents` searches a slice of the group per call: "This operation can return empty results while there are more log
//! events available through the token" (the API reference). A filter that matches little would otherwise read every event of the
//! group in one call, on the event loop; here a call reads at most [`SCAN_MAX`] rows and, when it stops there, answers a `nextToken`
//! at the last row it read, so the next call goes on from it.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use aws_wire::ApiError;

use crate::core::Core;
use crate::model::Stream;
use crate::reading::{Row, parse_row};

/// Rows one call reads at most, across all its streams.
pub(crate) const SCAN_MAX: usize = 10_000;

/// A position in the merged order: time, stream name, sequence.
pub(crate) type Pos = (i64, String, u64);

/// One stream being read: the rows fetched and not yet merged, and where the next fetch starts.
struct Cursor {
    no: u32,
    rows: VecDeque<Row>,
    from: String,
    to: String,
    done: bool,
}

/// What a merged read found: the rows kept, and where it stopped when it hit its bound before its end.
pub(crate) struct Merged<'s> {
    pub(crate) found: Vec<(Row, &'s Stream)>,
    pub(crate) resume: Option<Pos>,
}

impl Core {
    fn fill(&self, c: &mut Cursor, chunk: usize) -> Result<(), ApiError> {
        if !c.rows.is_empty() || c.done {
            return Ok(());
        }
        let got = self
            .events
            .scan(self.db.as_ref(), &c.from, &c.to, chunk, false)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        c.done = got.len() < chunk;
        if let Some((last, _)) = got.last() {
            c.from = format!("{last}\u{0}");
        }
        c.rows
            .extend(got.iter().filter_map(|(k, v)| parse_row(k, v)));
        if c.rows.is_empty() && !c.done {
            // WHY: a chunk of rows none of which could be read; the next fill reads on past them.
            return self.fill(c, chunk);
        }
        Ok(())
    }

    /// The rows of `streams` between the positions `range` (as in a token), after `after`, merged in order: at most `want` of those
    /// `keep` accepts, reading at most `budget` rows.
    pub(crate) fn merged<'s>(
        &self,
        streams: &'s [Stream],
        range: (&str, &str),
        after: Option<&Pos>,
        (want, budget): (usize, usize),
        keep: &dyn Fn(&Row) -> bool,
    ) -> Result<Merged<'s>, ApiError> {
        // A few rows a stream at a time when there are many streams, so what is held stays about the same.
        let chunk = (4_000 / streams.len().max(1)).clamp(8, 200);
        let mut cursors: Vec<Cursor> = streams
            .iter()
            .map(|s| Cursor {
                no: s.no,
                rows: VecDeque::new(),
                from: format!("{:08x}{}", s.no, range.0),
                to: format!("{:08x}{}", s.no, range.1),
                done: false,
            })
            .collect();
        let mut heads = BinaryHeap::new();
        for (i, c) in cursors.iter_mut().enumerate() {
            self.fill(c, chunk)?;
            if let (Some(r), Some(s)) = (c.rows.front(), streams.get(i)) {
                heads.push(Reverse((r.ts, s.name.as_str(), r.seq, i)));
            }
        }
        let mut out = Merged {
            found: Vec::new(),
            resume: None,
        };
        let mut scanned = 0;
        while let Some(Reverse((_, _, _, i))) = heads.pop() {
            let (Some(c), Some(s)) = (cursors.get_mut(i), streams.get(i)) else {
                break;
            };
            let Some(row) = c.rows.pop_front() else {
                continue;
            };
            debug_assert_eq!(c.no, s.no);
            self.fill(c, chunk)?;
            if let Some(next) = c.rows.front() {
                heads.push(Reverse((next.ts, s.name.as_str(), next.seq, i)));
            }
            let pos = (row.ts, s.name.as_str(), row.seq);
            let later = after.is_none_or(|(ts, name, seq)| pos > (*ts, name.as_str(), *seq));
            if !later {
                // WHY: rows at or before the token (those of its millisecond) are not counted, so a call always gets past them.
                continue;
            }
            scanned += 1;
            let (ts, seq) = (row.ts, row.seq);
            if keep(&row) {
                out.found.push((row, s));
                if out.found.len() >= want {
                    break;
                }
            }
            if scanned >= budget && !heads.is_empty() {
                out.resume = Some((ts, s.name.clone(), seq));
                break;
            }
        }
        Ok(out)
    }
}
