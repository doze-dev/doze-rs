//! What happens with the passing of time, done at the start of the next call (nothing wakes by itself): tables and indexes leave their
//! transitional states, and items whose time-to-live has passed are deleted.

use aws_expr::{Number, Value};
use aws_wire::ApiError;
use aws_wire::service::Call;

use crate::core::Core;
use crate::keys::{self, Space};
use crate::meta::Status;

/// The most expired items one sweep deletes; the next call carries on.
const SWEEP_BATCH: usize = 500;

impl Core {
    /// Settles transitional states and sweeps expired items, and waits for what that wrote.
    pub(crate) async fn housekeeping(&mut self, _call: &Call) -> Result<(), ApiError> {
        let now = self.clock.now_ms();
        self.settle(now);
        self.sweep_expired(now)?;
        let writes = std::mem::take(&mut self.pending);
        let mut writes = writes;
        match self.tables.stage() {
            Ok(changes) => writes.meta.push(changes),
            Err(e) => return Err(ApiError::internal(format!("could not save: {e}"))),
        }
        self.appended = false;
        if let Some(saved) = self.disk.submit(writes)
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        Ok(())
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
        for space in [Space::Items, Space::Index] {
            self.pending
                .ranges
                .push((space, t.id.to_vec(), keys::prefix_end(&t.id)));
        }
        if let Some(s) = &t.stream {
            self.pending
                .ranges
                .push((Space::Stream, s.id.to_vec(), keys::prefix_end(&s.id)));
            self.seqs.remove(&s.id);
        }
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
            if expired.len() >= SWEEP_BATCH {
                self.next_sweep_ms = now;
            }
            for pk in expired {
                self.expire(&t, &pk, now)?;
            }
        }
        Ok(())
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
