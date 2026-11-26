//! What happens with the passing of time: tables and indexes leave their transitional states, and items whose time-to-live has passed
//! are deleted. Both are done at the start of every call; the deletions are also done when they fall due, by the service's timer,
//! which sleeps until the first item of any table with time to live on expires ([`Core::ttl_due`]) and is not running while no such
//! item exists. So a table nobody calls still sees its items go, and its stream gets their REMOVE records.

use aws_expr::{Number, Value};
use aws_wire::ApiError;

use crate::core::Core;
use crate::keys::{self, Space};
use crate::meta::Status;
use crate::stage::item_key;

/// The most expired items one sweep deletes; the next call (or the timer, at once) carries on.
pub(crate) const SWEEP_BATCH: usize = 500;

impl Core {
    /// Settles transitional states and sweeps expired items, and waits for what that wrote. Answers whether a stream record was
    /// appended (a time-to-live deletion on a table with a stream), so consumers can be woken.
    pub(crate) async fn housekeeping(&mut self) -> Result<bool, ApiError> {
        let now = self.clock.now_ms();
        self.settle(now);
        self.expire_backups(now);
        self.sweep_expired(now)?;
        self.save().await?;
        Ok(std::mem::take(&mut self.appended))
    }

    /// When the timer should next sweep: the expiry of the first item of any table with time to live on, as the time-to-live index
    /// files them. `None` while there is no such item, so the service holds no timer. Read from disk after a call that wrote, and
    /// kept otherwise.
    pub(crate) fn ttl_due(&mut self) -> Option<i64> {
        if self.ttl_stale {
            self.ttl_stale = false;
            self.ttl_due = self.first_expiry();
        }
        self.ttl_due
    }

    fn first_expiry(&self) -> Option<i64> {
        let mut due: Option<i64> = None;
        for (_, t) in self
            .tables
            .iter()
            .filter(|(_, t)| t.ttl.enabled && t.status != Status::Deleting)
        {
            if let Some(at) = self.first_expiry_of(t) {
                due = Some(due.map_or(at, |d| d.min(at)));
            }
        }
        due
    }

    /// When the first item of `t` expires, as the time-to-live index files it.
    pub(crate) fn first_expiry_of(&self, t: &crate::meta::Table) -> Option<i64> {
        let mut lo = t.id.to_vec();
        lo.extend_from_slice(&0_u32.to_be_bytes());
        let mut first = None;
        // WHY: a failed read leaves this table out of the deadline; the next call's sweep still finds its items.
        let _ = self
            .disk
            .scan(Space::Index, &lo, &keys::prefix_end(&lo), false, |_, pk| {
                first = Some(pk.to_vec());
                crate::disk::Flow::Stop
            });
        let pk = first?;
        // An entry always moves with its item (`stage`); one that does not match it is left to the next call's sweep rather
        // than made a deadline the timer could never satisfy.
        match self.read_item(&item_key(t, &pk)) {
            Ok(Some(item)) => match item.get(&t.ttl.attribute) {
                Some(Value::N(n)) => n.floor_i64().map(|secs| secs.saturating_mul(1000)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Tables and indexes whose window has passed become active; a table being deleted goes, with its items.
    fn settle(&mut self, now: i64) {
        let due: Vec<(String, bool)> = self
            .tables
            .iter()
            .filter(|(_, t)| {
                (t.status != Status::Active && t.status_until_ms <= now)
                    || t.gsis
                        .iter()
                        .any(|i| i.status != Status::Active && i.status_until_ms <= now)
            })
            .map(|(name, t)| (name.to_owned(), t.status == Status::Deleting))
            .collect();
        for (name, deleting) in due {
            if deleting {
                self.drop_table(&name);
            } else if let Some(t) = self.tables.get_mut(&name) {
                if t.status != Status::Active && t.status_until_ms <= now {
                    t.status = Status::Active;
                }
                for i in &mut t.gsis {
                    if i.status != Status::Active && i.status_until_ms <= now {
                        i.status = Status::Active;
                    }
                }
            }
        }
    }

    /// Removes a table's record, its items, its index entries and its stream.
    pub(crate) fn drop_table(&mut self, name: &str) {
        let Some(t) = self.tables.remove(name) else {
            return;
        };
        self.drop_insights(name);
        for space in [Space::Items, Space::Index] {
            self.pending
                .ranges
                .push((space, t.id.to_vec(), keys::prefix_end(&t.id)));
        }
        if let Some(s) = &t.stream {
            self.drop_stream_records(&s.id);
        }
        // Its recovery log, streaming destinations and resource policies go with it; its backups stay (that is what they are for).
        self.drop_log(&t.id);
        self.extras.remove(&crate::extras::kinesis_key(name));
        self.extras.remove(&crate::extras::restored_key(name));
        self.drop_policies(name);
    }

    /// Deletes the items whose time-to-live attribute is in the past, as AWS does in its own time: a stream sees a removal by the
    /// service.
    fn sweep_expired(&mut self, now: i64) -> Result<(), ApiError> {
        if now < self.next_sweep_ms {
            return Ok(());
        }
        self.next_sweep_ms = now + 1000;
        let now_secs = now / 1000;
        let names: Vec<String> = self
            .tables
            .iter()
            .filter(|(_, t)| t.ttl.enabled && t.status != Status::Deleting)
            .map(|(n, _)| n.to_owned())
            .collect();
        for name in names {
            let t = self.table(&name)?;
            let expired = self.expired_keys(&t, now_secs)?;
            if expired.len() >= SWEEP_BATCH {
                self.next_sweep_ms = now;
            }
            for pk in expired {
                self.expire(&t, &pk, now)?;
            }
        }
        Ok(())
    }

    /// The keys of the items of `t` whose time to live is at or before `now_secs`, at most [`SWEEP_BATCH`] of them.
    pub(crate) fn expired_keys(
        &self,
        t: &crate::meta::Table,
        now_secs: i64,
    ) -> Result<Vec<Vec<u8>>, ApiError> {
        let mut lo = t.id.to_vec();
        lo.extend_from_slice(&0_u32.to_be_bytes());
        let mut hi = lo.clone();
        Number::from_i64(now_secs).order_key(&mut hi);
        let hi = keys::prefix_end(&hi);
        let mut expired = Vec::new();
        let read = self.disk.scan(Space::Index, &lo, &hi, false, |_, pk| {
            expired.push(pk.to_vec());
            if expired.len() >= SWEEP_BATCH {
                crate::disk::Flow::Stop
            } else {
                crate::disk::Flow::Continue
            }
        });
        read.map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(expired)
    }

    /// Deletes one expired item, if it is still there and still expired.
    fn expire(&mut self, t: &crate::meta::Table, pk: &[u8], now: i64) -> Result<(), ApiError> {
        let mut key = t.id.to_vec();
        key.extend_from_slice(pk);
        let Some(old) = self.read_item(&key)? else {
            return Ok(());
        };
        let due = matches!(
            old.get(&t.ttl.attribute),
            Some(Value::N(n)) if n.floor_i64().is_some_and(|secs| secs <= now / 1000)
        );
        if due {
            self.stage(t, now, Some(&old), None, true)?;
        }
        Ok(())
    }
}
