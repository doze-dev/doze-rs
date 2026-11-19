//! Point-in-time recovery: while it is on, every change of an item files the item as it was before, under the table's id and the
//! time of the change (the recovery log). The table as it was at a time is its items now with each item changed since put back as
//! it was then: [`Core::items_at`]. A restore, a full export and an incremental export read the table that way.
//!
//! The log is kept for [`LOG_MS`] (a day), whatever recovery period the table asks for: a laptop does not need five weeks of every
//! write, and the window a describe reports says so honestly (`EarliestRestorableDateTime` is when recovery was turned on, or a day
//! ago, whichever is later). Entries older than that are removed a few at a time as the table is written, and all of them when
//! recovery is turned off or the table is deleted. `doze-aws data` counts them with the table.

use std::collections::HashMap;

use aws_expr::Item;
use aws_wire::ApiError;
use doze_store::record;
use serde::{Deserialize, Serialize};

use crate::core::Core;
use crate::disk::Flow;
use crate::keys::{self, Space};
use crate::meta::Table;

/// How long the recovery log keeps a change.
pub(crate) const LOG_MS: i64 = 24 * 60 * 60 * 1000;
const ENTRY_VERSION: u8 = 1;
/// The most stale entries one write removes.
const TRIM_BATCH: usize = 100;

/// One change: the item's primary key, and the item before it (none when it was made).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    pk: Vec<u8>,
    old: Option<Item>,
}

/// One change as read back: when, which item, what it was before.
#[derive(Debug)]
pub(crate) struct Change {
    pub(crate) time_ms: i64,
    pub(crate) pk: Vec<u8>,
    pub(crate) old: Option<Item>,
}

/// The log key of a time: the table's id, then the time so that byte order is time order.
fn time_key(id: &[u8; 16], ms: i64) -> Vec<u8> {
    let mut k = id.to_vec();
    k.extend_from_slice(&(ms.max(0).cast_unsigned()).to_be_bytes());
    k
}

fn time_of(key: &[u8]) -> i64 {
    key.get(16..24)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map_or(0, |b| u64::from_be_bytes(b).cast_signed())
}

/// The earliest time a table can be restored to: when recovery was turned on, or the log's length ago.
pub(crate) fn earliest_ms(t: &Table, now_ms: i64) -> i64 {
    let period = t.pitr.days.clamp(1, 35) * 24 * 60 * 60 * 1000;
    t.pitr.enabled_ms.max(now_ms - period.min(LOG_MS))
}

impl Core {
    /// Files the change of one item in the recovery log, when the table keeps one, and removes entries past the log's length.
    pub(crate) fn log_change(
        &mut self,
        t: &Table,
        now_ms: i64,
        pk: &[u8],
        old: Option<&Item>,
    ) -> Result<(), ApiError> {
        if !t.pitr.enabled {
            return Ok(());
        }
        self.pitr_seq = self.pitr_seq.wrapping_add(1);
        let mut key = time_key(&t.id, now_ms);
        key.extend_from_slice(&self.pitr_seq.to_be_bytes());
        let entry = Entry {
            pk: pk.to_vec(),
            old: old.cloned(),
        };
        let bytes = record::encode("recovery log entry", ENTRY_VERSION, &entry)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        self.pending.put(Space::Pitr, key, bytes);
        self.trim_log(&t.id, now_ms)
    }

    fn trim_log(&mut self, id: &[u8; 16], now_ms: i64) -> Result<(), ApiError> {
        let mut stale = Vec::new();
        let hi = time_key(id, now_ms - LOG_MS);
        self.disk
            .scan(Space::Pitr, id, &hi, false, |k, _| {
                stale.push(k.to_vec());
                if stale.len() >= TRIM_BATCH {
                    Flow::Stop
                } else {
                    Flow::Continue
                }
            })
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for k in stale {
            self.pending.del(Space::Pitr, k);
        }
        Ok(())
    }

    /// Removes a table's whole recovery log.
    pub(crate) fn drop_log(&mut self, id: &[u8; 16]) {
        self.pending
            .ranges
            .push((Space::Pitr, id.to_vec(), keys::prefix_end(id)));
    }

    /// The changes of a table from `since_ms` (inclusive) up to `until_ms` (exclusive), oldest first.
    pub(crate) fn changes(
        &self,
        t: &Table,
        since_ms: i64,
        until_ms: Option<i64>,
    ) -> Result<Vec<Change>, ApiError> {
        let lo = time_key(&t.id, since_ms);
        let hi = until_ms.map_or_else(|| keys::prefix_end(&t.id), |u| time_key(&t.id, u));
        let mut out = Vec::new();
        let mut bad = None;
        self.disk
            .scan(Space::Pitr, &lo, &hi, false, |k, v| {
                match record::decode::<Entry>("recovery log entry", ENTRY_VERSION, v) {
                    Ok(e) => out.push(Change {
                        time_ms: time_of(k),
                        pk: e.pk,
                        old: e.old,
                    }),
                    Err(e) => bad = Some(e.to_string()),
                }
                Flow::Continue
            })
            .map_err(|e| ApiError::internal(e.to_string()))?;
        match bad {
            Some(e) => Err(ApiError::internal(e)),
            None => Ok(out),
        }
    }

    /// For each item changed at or after `since_ms`, what it was just before: the first change's old image.
    pub(crate) fn before(
        &self,
        t: &Table,
        since_ms: i64,
    ) -> Result<HashMap<Vec<u8>, Option<Item>>, ApiError> {
        let mut out = HashMap::new();
        for c in self.changes(t, since_ms, None)? {
            out.entry(c.pk).or_insert(c.old);
        }
        Ok(out)
    }

    /// The items of the table as they were at `at_ms` (every change at or before that time counted), in key order of today's
    /// items and then the ones gone since.
    pub(crate) fn items_at(&self, t: &Table, at_ms: i64) -> Result<Vec<Item>, ApiError> {
        let mut then = self.before(t, at_ms.saturating_add(1))?;
        let mut out = Vec::new();
        let mut bad = None;
        self.disk
            .scan(
                Space::Items,
                &t.id,
                &keys::prefix_end(&t.id),
                false,
                |k, v| {
                    let pk = k.get(16..).unwrap_or_default();
                    match then.remove(pk) {
                        Some(Some(old)) => out.push(old),
                        Some(None) => {}
                        None => match crate::stage::decode_item(v) {
                            Ok(item) => out.push(item),
                            Err(e) => bad = Some(e),
                        },
                    }
                    Flow::Continue
                },
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(e) = bad {
            return Err(e);
        }
        out.extend(then.into_values().flatten());
        Ok(out)
    }

    /// The bytes and entries a table's recovery log holds.
    pub(crate) fn log_size(&self, id: &[u8; 16]) -> Result<(u64, u64), doze_store::StoreError> {
        self.disk.measure(Space::Pitr, id, &keys::prefix_end(id))
    }
}
