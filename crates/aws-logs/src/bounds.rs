//! What a group keeps: its retention, and the local bounds of [`crate::Settings`] (an age and an event count), applied lazily when
//! the group is written or read. Nothing ticks.

use doze_store::record;

use crate::core::{Core, VERSION};
use crate::events::EVENT_OVERHEAD;
use crate::model::{Event, event_key, stream_key};

/// A group's events past their retention are removed at most this often.
const SWEEP_EVERY_MS: i64 = 60_000;

impl Core {
    /// Removes the events of `group` older than its retention or the local bound (`--logs-retention-days`), whichever is shorter
    /// (lazily: when the group is written or read, not more than once a minute).
    pub(crate) fn sweep(&mut self, group: &str) {
        let now = self.now_ms();
        let Some(g) = self.groups.get(group) else {
            return;
        };
        let by_group = g.retention_days.map(|days| now - days * 86_400_000);
        let local = self.keep_ms.map(|keep| now.saturating_sub(keep));
        let Some(cutoff) = by_group.into_iter().chain(local).max() else {
            return;
        };
        if self
            .swept
            .get(group)
            .is_some_and(|at| now - at < SWEEP_EVERY_MS)
        {
            return;
        }
        self.swept.insert(group.to_owned(), now);
        let keys: Vec<String> = self
            .streams
            .with_prefix(&stream_key(group, ""))
            .map(|(k, _)| k.to_owned())
            .collect();
        for key in keys {
            let Some(no) = self.streams.get(&key).map(|s| s.no) else {
                continue;
            };
            let (from, to) = (event_key(no, 0, 0), event_key(no, cutoff, 0));
            let (removed, count) = self.sum_bytes(&from, &to);
            if count == 0 {
                continue;
            }
            self.events.delete_range(&from, &to);
            if let Some(n) = self.held.get(&key).copied() {
                self.held.insert(key.clone(), n.saturating_sub(count));
            }
            if let Some(s) = self.streams.get_mut(&key) {
                s.bytes = (s.bytes - removed).max(0);
                if s.bytes == 0 {
                    s.first_ms = None;
                    s.last_ms = None;
                    s.last_ingested_ms = None;
                } else {
                    s.first_ms = s.first_ms.map(|f| f.max(cutoff));
                }
            }
        }
    }

    /// The bytes the events in `from..to` count as, and how many there are.
    fn sum_bytes(&self, from: &str, to: &str) -> (i64, u64) {
        let (mut total, mut count, mut at) = (0i64, 0u64, from.to_owned());
        while let Ok(rows) = self.events.scan(self.db.as_ref(), &at, to, 500, false) {
            let Some((last, _)) = rows.last() else {
                break;
            };
            at = format!("{last}\u{0}");
            count += rows.len() as u64;
            for (_, value) in &rows {
                if let Ok(e) = record::decode::<Event>("event", VERSION, value) {
                    total += i64::try_from(e.message.len() + EVENT_OVERHEAD).unwrap_or(0);
                }
            }
        }
        (total, count)
    }

    /// The events a stream holds: kept as a count, or, for a stream from before the count was kept, counted once.
    pub(crate) fn held_of(&mut self, key: &str) -> u64 {
        if let Some(n) = self.held.get(key) {
            return *n;
        }
        let Some(no) = self.streams.get(key).map(|s| s.no) else {
            return 0;
        };
        let (_, count) = self.sum_bytes(&event_key(no, 0, 0), &format!("{:08x}", no + 1));
        self.held.insert(key.to_owned(), count);
        count
    }

    /// Holds the group to `--logs-max-events`: past it, the oldest streams' oldest events go until the group is a tenth under the
    /// cap (so the writes that follow do not each remove some). A local bound, not AWS behaviour. Only what is already on disk is
    /// removed, so the batch just written stays.
    pub(crate) fn cap(&mut self, group: &str) {
        if self.max_events == 0 {
            return;
        }
        let keys: Vec<String> = self
            .streams
            .with_prefix(&stream_key(group, ""))
            .map(|(k, _)| k.to_owned())
            .collect();
        let mut total: u64 = keys.iter().map(|k| self.held_of(k)).sum();
        if total <= self.max_events {
            return;
        }
        let target = self.max_events - self.max_events / 10;
        let mut by_age: Vec<(i64, String)> = keys
            .into_iter()
            .filter_map(|k| {
                let first = self.streams.get(&k)?.first_ms?;
                Some((first, k))
            })
            .collect();
        by_age.sort();
        for (_, key) in by_age {
            if total <= target {
                break;
            }
            let Some(no) = self.streams.get(&key).map(|s| s.no) else {
                continue;
            };
            let end = format!("{:08x}", no + 1);
            let need = usize::try_from(total - target).unwrap_or(usize::MAX);
            let Ok(rows) =
                self.events
                    .scan(self.db.as_ref(), &event_key(no, 0, 0), &end, need, false)
            else {
                continue;
            };
            let Some((last, _)) = rows.last() else {
                continue;
            };
            let after = format!("{last}\u{0}");
            let removed: i64 = rows
                .iter()
                .filter_map(|(_, v)| record::decode::<Event>("event", VERSION, v).ok())
                .map(|e| i64::try_from(e.message.len() + EVENT_OVERHEAD).unwrap_or(0))
                .sum();
            let next = self
                .events
                .scan(self.db.as_ref(), &after, &end, 1, false)
                .ok()
                .and_then(|r| r.first().and_then(|(k, _)| ts_of(k)));
            self.events.delete_range(&event_key(no, 0, 0), &after);
            let gone = rows.len() as u64;
            total = total.saturating_sub(gone);
            let left = self.held_of(&key).saturating_sub(gone);
            self.held.insert(key.clone(), left);
            if let Some(s) = self.streams.get_mut(&key) {
                s.bytes = (s.bytes - removed).max(0);
                match next {
                    Some(at) => s.first_ms = Some(at),
                    None if s.bytes == 0 => {
                        s.first_ms = None;
                        s.last_ms = None;
                    }
                    None => {}
                }
            }
        }
    }
}

/// The time in an event's row key (stream, then time, then sequence, in fixed-width hex).
fn ts_of(key: &str) -> Option<i64> {
    key.get(8..24).and_then(|h| i64::from_str_radix(h, 16).ok())
}
