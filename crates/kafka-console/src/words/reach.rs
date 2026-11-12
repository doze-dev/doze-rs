//! The sentence under the Messages table: what the read did.

use crate::format::{count, number};
use crate::model::{MessagesView, Span};

/// What the read of the Messages page did, in a sentence under the toolbar (only when a filter is set).
#[must_use]
pub fn reach_sentence(v: &MessagesView) -> Vec<Span> {
    let r = &v.reach;
    let mut out = Vec::new();
    if r.returned == 0 {
        out.push(Span::text(match r.scanned {
            Some((n, true)) => format!(
                "No record matched. The search looked at all {} of the partition.",
                count(usize::try_from(n).unwrap_or(0), "record", "records")
            ),
            Some((n, false)) => format!(
                "No record matched in the {} the search looked at; it stopped at its limit.",
                count(
                    usize::try_from(n).unwrap_or(0),
                    "newest record",
                    "newest records"
                )
            ),
            None => "The fetch returned no records.".to_owned(),
        }));
        return out;
    }
    let (first, last) = (r.first_offset.unwrap_or(0), r.last_offset.unwrap_or(0));
    out.push(Span::text(match r.scanned {
        Some(_) => "The search found ".to_owned(),
        None => "The fetch returned ".to_owned(),
    }));
    out.push(Span::strong(number(i64::try_from(r.returned).unwrap_or(0))));
    out.push(Span::text(if r.returned == 1 {
        " record, offset "
    } else {
        " records, offsets "
    }));
    out.push(Span::strong(number(first)));
    if last != first {
        out.push(Span::text(" to "));
        out.push(Span::strong(number(last)));
    }
    out.push(Span::text(if r.reached_end && r.scanned.is_none() {
        ", the end of the partition."
    } else {
        "."
    }));
    if let Some((looked, complete)) = r.scanned {
        let n = number(i64::try_from(looked).unwrap_or(0));
        out.push(Span::text(if complete {
            format!(" It looked at {n} records, all there are.")
        } else {
            format!(" It looked at the newest {n} records and stopped at its limit; older ones may match.")
        }));
    }
    if r.above_lso > 0 && v.query.reading == crate::model::Reading::Uncommitted {
        let lso = v
            .query
            .partition
            .and_then(|p| v.head.parts.iter().find(|x| x.partition == p))
            .map(|p| format!(" ({})", number(p.last_stable)))
            .unwrap_or_default();
        out.push(Span::text(if r.above_lso == r.returned {
            format!(" All {} are at or above the last stable offset{lso}: a read_committed consumer would get none of them.", r.returned)
        } else {
            format!(" {} of them are at or above the last stable offset{lso}: a read_committed consumer would not get those.", r.above_lso)
        }));
    }
    out
}
