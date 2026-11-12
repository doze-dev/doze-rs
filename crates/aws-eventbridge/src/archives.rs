//! Archives: a copy of the events of a bus that match a pattern, kept for a replay. An event is a row of its own, keyed by archive and
//! time, so a replay reads only the window it asks for. Retention is enforced when an event is appended (at most once an hour for an
//! archive); the counts AWS reports are kept as events come and go.
//!
//! Wording of refusals not in the recordings is from memory.

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::event::Event;
use crate::model::{Archive, key};
use crate::names::{Parts, archive_arn};
use crate::page::{page, with_token};
use crate::pattern;
use crate::req::{Req, exists, invalid_pattern, not_found};
use crate::util::{Out, secs};

/// An hour: how often an archive's old events are looked for.
const PURGE_EVERY_MS: i64 = 3_600_000;
/// The most old events removed in one go.
const PURGE_BATCH: usize = 10_000;

/// The row key of an archived event: archive, time (zero-padded so rows sort by time), id.
pub(crate) fn event_key(account: &str, archive: &str, time_ms: i64, id: &str) -> String {
    format!("{account}/{archive}/{time_ms:020}/{id}")
}

impl Core {
    /// Appends the event to every archive of the bus that wants it. An event a replay put back is not archived again.
    pub(crate) fn archive_event(&mut self, account: &str, bus: &str, event: &Event) {
        if event.replay_name.is_some() {
            return;
        }
        let Some(bus_arn) = self.buses.get(&key(account, &[bus])).map(|b| b.arn.clone()) else {
            return;
        };
        let keys: Vec<String> = self
            .archives
            .with_prefix(&key(account, &[""]))
            .filter(|(_, a)| a.source_arn == bus_arn)
            .map(|(k, _)| k.to_owned())
            .collect();
        if keys.is_empty() {
            return;
        }
        let doc = event.doc();
        let text = event.text();
        for k in keys {
            self.append(&k, event, &doc, &text);
        }
    }

    fn append(&mut self, archive_key: &str, event: &Event, doc: &Value, text: &str) {
        let now = self.now_ms();
        let Some(a) = self.archives.get(archive_key) else {
            return;
        };
        let wanted = a
            .pattern
            .as_deref()
            .and_then(|p| pattern::parse(p).ok())
            .is_none_or(|p| pattern::matches(&p, doc));
        if !wanted {
            return;
        }
        self.purge(archive_key, now);
        let Some(a) = self.archives.get_mut(archive_key) else {
            return;
        };
        let row = event_key(&a.account, &a.name, event.time_ms, &event.id);
        a.events += 1;
        a.bytes += text.len() as u64;
        self.archived.put(row, text.as_bytes().to_vec());
    }

    /// Removes the events older than the archive's retention, if it has one and an hour has gone by since last time.
    fn purge(&mut self, archive_key: &str, now: i64) {
        let Some(a) = self.archives.get(archive_key) else {
            return;
        };
        if a.retention_days <= 0
            || now - self.purged.get(archive_key).copied().unwrap_or(0) < PURGE_EVERY_MS
        {
            return;
        }
        let (from, to) = (
            event_key(&a.account, &a.name, 0, ""),
            event_key(&a.account, &a.name, now - a.retention_days * 86_400_000, ""),
        );
        self.purged.insert(archive_key.to_owned(), now);
        let Ok(old) = self
            .archived
            .scan(self.db.as_ref(), &from, &to, PURGE_BATCH, false)
        else {
            return;
        };
        let (count, bytes) = (
            old.len() as u64,
            old.iter().map(|(_, v)| v.len() as u64).sum::<u64>(),
        );
        for (row, _) in &old {
            self.archived.delete(row);
        }
        if let Some(a) = self.archives.get_mut(archive_key) {
            a.events = a.events.saturating_sub(count);
            a.bytes = a.bytes.saturating_sub(bytes);
        }
    }

    fn archive(&self, r: &Req<'_>, name: &str) -> Result<&Archive, ApiError> {
        self.archives
            .get(&key(r.account(), &[name]))
            .ok_or_else(|| not_found(format!("Archive {name} does not exist.")))
    }

    pub(crate) fn create_archive(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ArchiveName");
        let k = key(r.account(), &[name]);
        if self.archives.get(&k).is_some() {
            return Err(exists(format!("Archive {name} already exists.")));
        }
        let source = r.s("EventSourceArn");
        let bus = Parts::of(source)
            .filter(|p| p.kind == "event-bus")
            .map(|p| p.tail)
            .ok_or_else(|| crate::req::invalid("Parameter EventSourceArn is not valid."))?;
        self.bus(r.account(), Some(bus))?;
        if let Some(text) = r.opt("EventPattern") {
            pattern::parse(text).map_err(|e| invalid_pattern(e.message(text)))?;
        }
        let a = Archive {
            account: r.account().to_owned(),
            name: name.to_owned(),
            arn: archive_arn(r.region(), r.account(), name),
            source_arn: source.to_owned(),
            description: r.opt("Description").map(str::to_owned),
            pattern: r.opt("EventPattern").map(str::to_owned),
            retention_days: r.int("RetentionDays").unwrap_or(0),
            kms_key: r.opt("KmsKeyIdentifier").map(str::to_owned),
            created_ms: self.now_ms(),
            events: 0,
            bytes: 0,
        };
        let out = changed(&a);
        self.archives.insert(k, a);
        Ok(out)
    }

    pub(crate) fn describe_archive(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let a = self.archive(r, r.s("ArchiveName"))?;
        Ok(Out::new()
            .with("ArchiveArn", a.arn.clone())
            .with("ArchiveName", a.name.clone())
            .with("EventSourceArn", a.source_arn.clone())
            .maybe("Description", a.description.clone())
            .maybe("EventPattern", a.pattern.clone())
            .with("State", "ENABLED")
            .maybe("KmsKeyIdentifier", a.kms_key.clone())
            .with("RetentionDays", a.retention_days)
            .with("SizeBytes", a.bytes)
            .with("EventCount", a.events)
            .with("CreationTime", secs(a.created_ms))
            .done())
    }

    pub(crate) fn list_archives(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let source = r.opt("EventSourceArn");
        let state = r.opt("State");
        let mine = key(r.account(), &[""]);
        let items = self
            .archives
            .with_prefix(&mine)
            .filter(|(_, a)| a.name.starts_with(prefix))
            .filter(|(_, a)| source.is_none_or(|s| a.source_arn == s))
            .filter(|_| state.is_none_or(|s| s == "ENABLED"))
            .map(|(k, a)| (k.to_owned(), a));
        let (archives, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = archives
            .into_iter()
            .map(|a| {
                Out::new()
                    .with("ArchiveName", a.name.clone())
                    .with("EventSourceArn", a.source_arn.clone())
                    .with("State", "ENABLED")
                    .with("RetentionDays", a.retention_days)
                    .with("SizeBytes", a.bytes)
                    .with("EventCount", a.events)
                    .with("CreationTime", secs(a.created_ms))
                    .done()
            })
            .collect();
        Ok(with_token(Out::new().with("Archives", list).done(), token))
    }

    pub(crate) fn update_archive(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.archive(r, r.s("ArchiveName"))?.name.clone();
        if let Some(text) = r.opt("EventPattern") {
            pattern::parse(text).map_err(|e| invalid_pattern(e.message(text)))?;
        }
        let Some(a) = self.archives.get_mut(&key(r.account(), &[&name])) else {
            return Err(not_found(format!("Archive {name} does not exist.")));
        };
        if let Some(d) = r.opt("Description") {
            a.description = Some(d.to_owned());
        }
        if let Some(p) = r.opt("EventPattern") {
            a.pattern = Some(p.to_owned());
        }
        if let Some(days) = r.int("RetentionDays") {
            a.retention_days = days;
        }
        if let Some(k) = r.opt("KmsKeyIdentifier") {
            a.kms_key = Some(k.to_owned());
        }
        Ok(changed(a))
    }

    pub(crate) fn delete_archive(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.archive(r, r.s("ArchiveName"))?.name.clone();
        let k = key(r.account(), &[&name]);
        self.archives.remove(&k);
        self.purged.remove(&k);
        let (from, to) = (format!("{k}/"), format!("{k}0"));
        self.archived.delete_range(&from, &to);
        Ok(Value::Null)
    }
}

/// The answer of create and update.
fn changed(a: &Archive) -> Value {
    Out::new()
        .with("ArchiveArn", a.arn.clone())
        .with("State", "ENABLED")
        .with("CreationTime", secs(a.created_ms))
        .done()
}
