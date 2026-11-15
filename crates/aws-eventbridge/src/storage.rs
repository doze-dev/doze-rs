//! EventBridge's data for a person to see and remove (`doze-aws data`). A resource is an archive; its bytes are its archived
//! events' rows. Emptying removes the events, whole or those before a time, and keeps the archive with its pattern and retention
//! (its `EventCount` and `SizeBytes` drop to match). Buses, rules, targets, connections, API destinations and replays are
//! definitions: emptying never touches them; a reset removes them, and the default bus comes back on the next call, as on a fresh
//! data directory.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use tokio::sync::oneshot;

use crate::Eventbridge;
use crate::archives::event_key;
use crate::core::{Core, EVENTS, Request};
use crate::model::Archive;

/// What the actor is asked about its data.
#[derive(Debug)]
pub(crate) enum DataAsk {
    Usage,
    Empty { name: String, prune: Prune },
    Reset { dry_run: bool },
    Compact,
}

/// Its answer.
#[derive(Debug)]
pub(crate) enum DataReply {
    Usage(Vec<Resource>, File),
    Emptied(Result<Emptied, StorageError>),
    Compacted(Result<u64, String>),
}

/// A data question and where its answer goes.
#[derive(Debug)]
pub(crate) struct DataCall {
    pub(crate) region: String,
    pub(crate) ask: DataAsk,
    pub(crate) reply: oneshot::Sender<DataReply>,
}

/// The rows of an archive from its start up to `before` (all of them when `None`).
fn range(a: &Archive, before: Option<i64>) -> (String, String) {
    let from = event_key(&a.account, &a.name, 0, "");
    let to = before.map_or_else(
        || format!("{}/{}0", a.account, a.name),
        |b| event_key(&a.account, &a.name, b.max(0), ""),
    );
    (from, to)
}

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region), self.file(region)),
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune)),
            DataAsk::Reset { dry_run } => DataReply::Emptied(Ok(self.reset(dry_run))),
            DataAsk::Compact => DataReply::Compacted(self.compact()),
        }
    }

    /// The rows in `from..to`, the bytes of their keys and values, and of the values alone (what `SizeBytes` counts).
    fn rows_in(&self, from: &str, to: &str) -> (u64, u64, u64) {
        let mut done = (0, 0, 0);
        match &self.db {
            Some(db) => {
                // WHY: a failed read reports what was counted; the next usage reads again.
                drop(db.scan(EVENTS, from, |k, v| {
                    if k >= to {
                        return false;
                    }
                    done.0 += 1;
                    done.1 += (k.len() + v.len()) as u64;
                    done.2 += v.len() as u64;
                    true
                }));
            }
            None => {
                for (k, v) in self
                    .archived
                    .scan(None, from, to, usize::MAX, false)
                    .unwrap_or_default()
                {
                    done.0 += 1;
                    done.1 += (k.len() + v.len()) as u64;
                    done.2 += v.len() as u64;
                }
            }
        }
        done
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        self.archives
            .iter()
            .map(|(_, a)| {
                let (from, to) = range(a, None);
                let (items, bytes, _) = self.rows_in(&from, &to);
                let mut notes = Vec::new();
                if items > 0 && a.retention_days <= 0 {
                    notes.push(Note {
                        bytes,
                        text: "no retention set: its events are kept forever".to_owned(),
                        prune: Some(Prune::default()),
                    });
                }
                Resource {
                    region: region.to_owned(),
                    name: a.name.clone(),
                    bytes,
                    items,
                    notes,
                }
            })
            .collect()
    }

    fn file(&self, region: &str) -> File {
        let space = self
            .db
            .as_ref()
            .and_then(|db| db.space().ok())
            .unwrap_or_default();
        File {
            region: region.to_owned(),
            bytes: space.file,
            reclaimable: space.reclaimable(),
        }
    }

    fn empty(&mut self, name: &str, prune: &Prune) -> Result<Emptied, StorageError> {
        if prune.prefix.is_some() || prune.old_versions || prune.unused {
            return Err(StorageError::Unsupported(
                "an archive can be emptied whole, or of the events before a time (--older-than)"
                    .to_owned(),
            ));
        }
        let keys: Vec<String> = self
            .archives
            .iter()
            .filter(|(_, a)| a.name == name)
            .map(|(k, _)| k.to_owned())
            .collect();
        if keys.is_empty() {
            return Err(StorageError::NotFound);
        }
        let mut done = Emptied::default();
        let mut ranges = Vec::new();
        for k in &keys {
            let Some(a) = self.archives.get(k) else {
                continue;
            };
            let (from, to) = range(a, prune.before_ms);
            let (items, bytes, text) = self.rows_in(&from, &to);
            done.items += items;
            done.bytes += bytes;
            ranges.push((k.clone(), from, to, items, text));
        }
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        for (k, from, to, items, text) in ranges {
            self.archived.delete_range(&from, &to);
            if let Some(a) = self.archives.get_mut(&k) {
                a.events = a.events.saturating_sub(items);
                a.bytes = a.bytes.saturating_sub(text);
            }
        }
        done.freed = self.settle_data().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn reset(&mut self, dry_run: bool) -> Emptied {
        let mut done = Emptied::default();
        for (_, a) in self.archives.iter() {
            let (from, to) = range(a, None);
            let (items, bytes, _) = self.rows_in(&from, &to);
            done.items += items;
            done.bytes += bytes;
        }
        if dry_run {
            return done;
        }
        clear(&mut self.buses);
        clear(&mut self.rules);
        clear(&mut self.archives);
        clear(&mut self.replays);
        clear(&mut self.connections);
        clear(&mut self.destinations);
        self.archived.delete_range("", "\u{10ffff}");
        self.compiled.clear();
        self.due.clear();
        clear(&mut self.pending);
        clear(&mut self.retries);
        self.purged.clear();
        self.last_caller = None;
        // WHY: a failed write is the writer's to report on the next call; the counts say what was asked.
        done.freed = self.settle_data().unwrap_or(0);
        done
    }

    /// Writes what changed, waits for it to be on disk, and compacts; answers the bytes the disk got back.
    fn settle_data(&mut self) -> Result<u64, String> {
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.flush().map_err(|e| e.to_string())?);
        self.compact()
    }

    fn compact(&mut self) -> Result<u64, String> {
        match self.db.as_mut() {
            Some(db) => db.compact().map(|c| c.freed()).map_err(|e| e.to_string()),
            None => Ok(0),
        }
    }
}

/// Removes every record of a table.
fn clear<T: serde::Serialize + serde::de::DeserializeOwned>(
    records: &mut doze_store::records::Records<T>,
) {
    let keys: Vec<String> = records.iter().map(|(k, _)| k.to_owned()).collect();
    for key in keys {
        records.remove(&key);
    }
}

impl Eventbridge {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let actor = self.actors.actor(region).ok()?;
        let (reply, answer) = oneshot::channel();
        let call = DataCall {
            region: region.to_owned(),
            ask,
            reply,
        };
        actor.call(Request::Data(Box::new(call))).await.ok()?;
        answer.await.ok()
    }

    fn regions(&self) -> Vec<String> {
        self.actors.keys_on_disk()
    }
}

impl Storage for Eventbridge {
    fn names(&self) -> Names {
        Names {
            one: "archive",
            many: "archives",
            units: "events",
            prunes: &["before"],
        }
    }

    fn usage(&self) -> BoxFuture<'_, Result<Usage, String>> {
        Box::pin(async move {
            let mut usage = Usage::default();
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Usage).await {
                    Some(DataReply::Usage(resources, file)) => {
                        usage.resources.extend(resources);
                        usage.files.push(file);
                    }
                    _ => return Err(format!("EventBridge in {region} did not answer")),
                }
            }
            Ok(usage)
        })
    }

    fn empty<'a>(
        &'a self,
        region: &'a str,
        name: &'a str,
        prune: &'a Prune,
    ) -> BoxFuture<'a, Result<Emptied, StorageError>> {
        Box::pin(async move {
            if !self.regions().iter().any(|r| r == region) {
                return Err(StorageError::NotFound);
            }
            let ask = DataAsk::Empty {
                name: name.to_owned(),
                prune: prune.clone(),
            };
            match self.ask_data(region, ask).await {
                Some(DataReply::Emptied(done)) => done,
                _ => Err(StorageError::Failed(format!(
                    "EventBridge in {region} did not answer"
                ))),
            }
        })
    }

    fn reset(&self, dry_run: bool) -> BoxFuture<'_, Result<Emptied, String>> {
        Box::pin(async move {
            let mut total = Emptied::default();
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Reset { dry_run }).await {
                    Some(DataReply::Emptied(Ok(done))) => total.add(done),
                    _ => return Err(format!("EventBridge in {region} did not answer")),
                }
            }
            Ok(total)
        })
    }

    fn compact(&self) -> BoxFuture<'_, Result<u64, String>> {
        Box::pin(async move {
            let mut freed = 0;
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Compact).await {
                    Some(DataReply::Compacted(Ok(n))) => freed += n,
                    Some(DataReply::Compacted(Err(e))) => return Err(e),
                    _ => return Err(format!("EventBridge in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
