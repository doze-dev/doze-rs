//! Reading the records of a batch to show them: offset, timestamp, key, value and headers, through whatever codec the
//! batch uses, held to a limit on what may be inflated. For display only: nothing here is written back, and a record
//! that cannot be read ends the walk with an error rather than a guess.

use super::recode::read_varint;
use super::{Batch, Refusal, RecordsRefused};
use crate::ids::Offset;

/// The most headers one record is read with; a record that claims more is unreadable (it cannot hold them).
const MOST_HEADERS: usize = 1 << 16;

/// One header of a record. A header's value may be absent.
pub type LookedHeader<'a> = (&'a [u8], Option<&'a [u8]>);

/// One record as a reader sees it. Borrowed from the inflated section, so it lives for the callback only.
#[derive(Debug, Clone)]
pub struct Looked<'a> {
    /// Its offset in the log.
    pub offset: i64,
    /// Its timestamp (the batch's first timestamp plus the record's delta).
    pub timestamp: i64,
    /// Its key, if it has one.
    pub key: Option<&'a [u8]>,
    /// Its value; `None` is a deletion (a tombstone).
    pub value: Option<&'a [u8]>,
    /// Its headers, in order.
    pub headers: Vec<LookedHeader<'a>>,
}

/// A field of `len` bytes (a negative length is none) off the front of `rest`.
fn field<'a>(rest: &mut &'a [u8]) -> Option<Option<&'a [u8]>> {
    let (len, used) = read_varint(rest)?;
    *rest = rest.get(used..)?;
    if len < 0 {
        return Some(None);
    }
    let len = usize::try_from(len).ok()?;
    let (got, after) = (rest.get(..len)?, rest.get(len..)?);
    *rest = after;
    Some(Some(got))
}

fn headers<'a>(rest: &mut &'a [u8]) -> Option<Vec<LookedHeader<'a>>> {
    let (count, used) = read_varint(rest)?;
    *rest = rest.get(used..)?;
    let count = usize::try_from(count).ok().filter(|c| *c <= MOST_HEADERS)?;
    // A header is at least two length bytes, so a count the rest cannot hold is no count.
    if count > rest.len() / 2 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let key = field(rest)??;
        out.push((key, field(rest)?));
    }
    Some(out)
}

/// One record (without its length prefix) read in full, its offset worked out from `base`.
fn parse<'a>(record: &'a [u8], base: i64, first_timestamp: i64) -> Option<Looked<'a>> {
    let mut rest = record.get(1..)?; // the record's attribute byte
    let (timestamp_delta, n) = read_varint(rest)?;
    rest = rest.get(n..)?;
    let (offset_delta, n) = read_varint(rest)?;
    rest = rest.get(n..)?;
    let key = field(&mut rest)?;
    let value = field(&mut rest)?;
    let headers = headers(&mut rest)?;
    rest.is_empty().then_some(Looked {
        offset: base.saturating_add(offset_delta),
        timestamp: first_timestamp.saturating_add(timestamp_delta),
        key,
        value,
        headers,
    })
}

impl Batch {
    // fuzz:parser fuzz_batch
    /// Calls `look` with each record of the batch, in order, until it returns `false`. The records are read through the
    /// batch's codec; inflating past `max_inflated` bytes is refused.
    ///
    /// # Errors
    /// A records section that cannot be read, or one that inflates past `max_inflated`.
    pub fn each_record_full(
        &self,
        max_inflated: u64,
        mut look: impl FnMut(&Looked<'_>) -> bool,
    ) -> Result<(), RecordsRefused> {
        let unreadable =
            |why: &str| RecordsRefused::new(Refusal::UnknownServerError, false, why.to_owned());
        let inflated = self.inflated(max_inflated)?;
        let base = self.base_offset().map_or(0, Offset::get);
        let first = self.first_timestamp().get();
        let mut rest = inflated.as_slice();
        for _ in 0..self.record_count() {
            let (length, used) =
                read_varint(rest).ok_or_else(|| unreadable("a record is cut short"))?;
            let end = usize::try_from(length)
                .ok()
                .and_then(|l| used.checked_add(l))
                .ok_or_else(|| unreadable("a record is cut short"))?;
            let record = rest
                .get(used..end)
                .ok_or_else(|| unreadable("a record is cut short"))?;
            rest = rest.get(end..).unwrap_or_default();
            let looked = parse(record, base, first).ok_or_else(|| unreadable("a record cannot be read"))?;
            if !look(&looked) {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
