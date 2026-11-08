//! A read, checked against what the workload knows: the batch that holds the offset first, then the ones after it,
//! whole, in order, within the byte limit and below the limit offset, and stopping early only for a reason.

use bytes::Bytes;
use kafka_wire::batch::Batches;

use kafka_wire::ids::Offset;

use crate::model::Model;
use crate::world::{World, offset};

/// What was asked of a read.
pub(crate) struct Ask {
    pub from: i64,
    pub max_bytes: usize,
    pub limit: Option<i64>,
    pub end: i64,
    /// The base offsets of the segments, which a read does not cross.
    pub segment_bases: Vec<i64>,
}

pub(crate) fn check(model: &Model, ask: &Ask, got: &Bytes) -> Result<(), String> {
    // The batches the workload wrote, from the one that holds `from`.
    let from = ask.from;
    let holding = model
        .acked
        .range(..=from)
        .next_back()
        .filter(|(base, r)| *base + i64::from(r.count) > from);
    let first = holding.map(|(base, _)| *base).unwrap_or(from);
    let expected: Vec<_> = model
        .acked
        .range(first..)
        .map(|(base, r)| (*base, r.stamped(*base)))
        .collect();
    let served: Vec<_> = Batches::new(got.clone())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("a damaged batch was served: {e}"))?;
    let total = served_are_the_expected(&served, &expected, ask.max_bytes)?;
    stopped_for_a_reason(ask, first, &expected, served.len(), total)
}

/// The served batches are exactly the first of the expected ones, within the byte limit (but for a first batch
/// that is bigger than it). Returns their size.
fn served_are_the_expected(
    served: &[kafka_wire::batch::Batch],
    expected: &[(i64, Bytes)],
    max_bytes: usize,
) -> Result<usize, String> {
    let mut total = 0usize;
    for (i, batch) in served.iter().enumerate() {
        let base = batch.base_offset().map_or(-1, |b| b.get());
        let Some((want_base, want)) = expected.get(i) else {
            return Err(format!(
                "batch {base} was served and the workload wrote nothing there"
            ));
        };
        if base != *want_base || batch.as_bytes() != want {
            return Err(format!(
                "served batch {i} is {base}, expected {want_base} (or its bytes differ)"
            ));
        }
        total += batch.len();
    }
    if total > max_bytes && served.len() > 1 {
        return Err(format!(
            "{total} bytes in {} batches for a limit of {max_bytes}",
            served.len()
        ));
    }
    Ok(total)
}

/// A read may stop before the end of what is there only for a reason: the end, the limit, the byte limit, or the
/// edge of a segment (a read does not cross one).
fn stopped_for_a_reason(
    ask: &Ask,
    first: i64,
    expected: &[(i64, Bytes)],
    served: usize,
    total: usize,
) -> Result<(), String> {
    let Ask {
        from,
        max_bytes,
        limit,
        end,
        ..
    } = *ask;
    let next = expected.get(served);
    let reason = next.is_none_or(|(base, bytes)| {
        *base >= end
            || limit.is_some_and(|l| *base >= l)
            || (served > 0 && total + bytes.len() > max_bytes)
            || ask.segment_bases.contains(base)
    });
    if !reason {
        return Err(format!(
            "stopped after {served} batches for no reason (next is {:?})",
            next.map(|n| n.0)
        ));
    }
    let something_to_serve = expected
        .first()
        .is_some_and(|(base, _)| limit.is_none_or(|l| *base < l));
    if served == 0 && from < end && something_to_serve && !ask.segment_bases.contains(&first) {
        return Err(format!(
            "nothing was served for {from}, below the end {end}"
        ));
    }
    Ok(())
}

impl World {
    pub(crate) fn read(&mut self) {
        let Some(log) = self.log.as_ref() else { return };
        let (start, end) = (log.start_offset().get(), log.end_offset().get());
        let from = start + self.rng.below((end - start) as u64 + 1) as i64;
        let max_bytes = [1, 70, 200, 1000, usize::MAX][self.rng.below(5) as usize];
        // A limit (the last stable offset of a read-committed fetch) some of the time, below or above the offset.
        let limit = (self.rng.below(3) == 0).then(|| from - 3 + self.rng.below(12) as i64);
        let got = log
            .reader()
            .read(offset(from), max_bytes, limit.and_then(Offset::new));
        let got = match got {
            Ok(got) => got,
            Err(e) if e.to_string().contains("the process was stopped") => return,
            Err(e) => panic!("seed {}: a read of {from}..{end} failed: {e}", self.seed),
        };
        let segment_bases = self
            .fs
            .names(&self.dir)
            .iter()
            .filter_map(|n| n.strip_suffix(".log")?.parse().ok())
            .collect();
        let ask = crate::reads::Ask {
            from,
            max_bytes,
            limit,
            end,
            segment_bases,
        };
        if let Err(e) = crate::reads::check(&self.model, &ask, &got) {
            let near: Vec<_> = self
                .model
                .acked
                .range(from - 8..from + 4)
                .map(|(b, r)| (*b, r.count))
                .collect();
            let steps: Vec<_> = self.trail.iter().rev().take(6).collect();
            panic!(
                "seed {}: a read of {from} (max {max_bytes}, limit {limit:?}, end {end}): {e}\n  acknowledged near it: {near:?}\n  recent steps: {steps:?}",
                self.seed
            );
        }
    }
}
