//! CloudWatch Logs' data for a person to see and remove (`doze-aws data`): each log group's events, in all its streams, emptied
//! whole or of the events stamped before a time, and the database compacted after, so the disk gets the space back.
//!
//! A group with events and no retention says so: nothing removes them, and the groups Lambda writes to are made without anyone
//! choosing a retention. The note carries the prune that keeps the last week.
//!
//! Emptying keeps the group with everything set on it (retention, key, protection, tags, metric and subscription filters) and its
//! streams, with no events in them, as AWS leaves a stream whose events have expired; a stream's sequence token goes on moving.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::{Db, StoreError};
use tokio::sync::oneshot;

use crate::Logs;
use crate::core::{Ask, Core, EVENTS};
use crate::events::EVENT_OVERHEAD;
use crate::model::{Event, Group, event_key, stream_key};

/// How far back the note on a group with no retention suggests keeping events, when no local bound is set.
const SUGGEST_MS: i64 = 7 * DAY_MS;
const DAY_MS: i64 = 86_400_000;

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
    Usage(Result<(Vec<Resource>, File), String>),
    Emptied(Result<Emptied, StorageError>),
    Compacted(Result<u64, String>),
}

/// A data question and where its answer goes.
#[derive(Debug)]
pub(crate) struct Asked {
    pub(crate) region: String,
    pub(crate) ask: DataAsk,
    pub(crate) reply: oneshot::Sender<DataReply>,
}

/// What a stream's rows hold, all of them and those stamped before a time.
#[derive(Clone, Copy, Debug, Default)]
struct Held {
    rows: u64,
    bytes: u64,
    old_rows: u64,
    old_bytes: u64,
    /// The old events as AWS counts them (the message and 26), which the stream's `storedBytes` holds.
    old_counted: i64,
    /// The time of the first event kept.
    first_kept: Option<i64>,
}

impl Held {
    fn add(&mut self, other: Self) {
        self.rows += other.rows;
        self.bytes += other.bytes;
        self.old_rows += other.old_rows;
        self.old_bytes += other.old_bytes;
    }
}

/// The time in an event's row key (stream, then time, then sequence, in fixed-width hex).
fn ts_of(key: &str) -> i64 {
    key.get(8..24)
        .and_then(|h| i64::from_str_radix(h, 16).ok())
        .unwrap_or(0)
}

/// The keys of a group's streams.
fn stream_keys(core: &Core, group: &str) -> Vec<(String, u32)> {
    core.streams
        .with_prefix(&stream_key(group, ""))
        .map(|(k, s)| (k.to_owned(), s.no))
        .collect()
}

impl Core {
    /// Answers a data question. Deletes are handed to the writer before the file is compacted, and compaction waits for them.
    pub(crate) fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region).map_err(|e| e.to_string())),
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune)),
            DataAsk::Reset { dry_run } => {
                DataReply::Emptied(self.reset(dry_run).map_err(StorageError::Failed))
            }
            DataAsk::Compact => DataReply::Compacted(self.compact_file()),
        }
    }

    /// What stream `no` holds, and how much of it is stamped before `cut`.
    fn held(&self, no: u32, cut: i64) -> Result<Held, StoreError> {
        let mut held = Held::default();
        let Some(db) = &self.db else {
            return Ok(held);
        };
        let (from, to) = (format!("{no:08x}"), format!("{:08x}", no.saturating_add(1)));
        db.scan(EVENTS, &from, |k, v| {
            if k >= to.as_str() {
                return false;
            }
            let size = (k.len() + v.len()) as u64;
            held.rows += 1;
            held.bytes += size;
            let ts = ts_of(k);
            if ts < cut {
                held.old_rows += 1;
                held.old_bytes += size;
                if let Ok(e) = Event::decode(v) {
                    held.old_counted +=
                        i64::try_from(e.message.len() + EVENT_OVERHEAD).unwrap_or(0);
                }
            } else if held.first_kept.is_none() {
                held.first_kept = Some(ts);
            }
            true
        })?;
        Ok(held)
    }

    fn group_held(&self, group: &str, cut: i64) -> Result<Held, StoreError> {
        let mut total = Held::default();
        for (_, no) in stream_keys(self, group) {
            total.add(self.held(no, cut)?);
        }
        Ok(total)
    }

    fn usage(&self, region: &str) -> Result<(Vec<Resource>, File), StoreError> {
        let mut out = Vec::new();
        for (name, g) in self.groups.iter() {
            let (cut, text) = self.kept_from(g);
            let held = self.group_held(name, cut.unwrap_or(i64::MIN))?;
            let mut notes = Vec::new();
            if let (Some(cut), Some(text)) = (cut, text)
                && held.rows > 0
                && (g.retention_days.is_none() || held.old_rows > 0)
            {
                notes.push(Note {
                    bytes: held.old_bytes,
                    text,
                    prune: Some(Prune {
                        before_ms: Some(cut),
                        ..Prune::default()
                    }),
                });
            }
            out.push(Resource {
                region: region.to_owned(),
                name: name.to_owned(),
                bytes: held.bytes,
                items: held.rows,
                notes,
            });
        }
        let space = self
            .db
            .as_ref()
            .map(Db::space)
            .transpose()?
            .unwrap_or_default();
        let file = File {
            region: region.to_owned(),
            bytes: space.file,
            reclaimable: space.reclaimable(),
        };
        Ok((out, file))
    }

    /// What is worth saying about how long a group keeps its events, and the time before which a prune would remove what it says.
    /// A group with no retention keeps its events forever on AWS; here the local bound (`--logs-retention-days`, 7 days unless
    /// changed) removes them, but only when the group is next written or read, so one nothing touches keeps them too. A group with a
    /// retention is pointed at only while events past it wait for that next touch.
    fn kept_from(&self, g: &Group) -> (Option<i64>, Option<String>) {
        let now = self.now_ms();
        let by_group = g.retention_days.map(|days| now - days * DAY_MS);
        let local = self.keep_ms.map(|keep| now.saturating_sub(keep));
        match (g.retention_days, self.keep_ms) {
            (None, None) => (
                Some(now - SUGGEST_MS),
                Some("no retention set: kept forever".to_owned()),
            ),
            (None, Some(keep)) => (
                local,
                Some(format!(
                    "no retention set: kept forever on AWS; doze removes events older than {} days (--logs-retention-days) when the group is next written or read",
                    keep / DAY_MS
                )),
            ),
            (Some(_), _) => (
                by_group.into_iter().chain(local).max(),
                Some(
                    "events past the retention, removed when the group is next written or read"
                        .to_owned(),
                ),
            ),
        }
    }

    fn empty(&mut self, name: &str, prune: &Prune) -> Result<Emptied, StorageError> {
        if prune.prefix.is_some() || prune.old_versions || prune.unused {
            return Err(StorageError::Unsupported(
                "a log group can be emptied whole, or of the events stamped before a time (--older-than)".to_owned(),
            ));
        }
        if self.groups.get(name).is_none() {
            return Err(StorageError::NotFound);
        }
        let cut = prune.before_ms.unwrap_or(i64::MAX);
        let mut done = Emptied::default();
        let mut changed = Vec::new();
        for (key, no) in stream_keys(self, name) {
            let held = self
                .held(no, cut)
                .map_err(|e| StorageError::Failed(e.to_string()))?;
            done.items += held.old_rows;
            done.bytes += held.old_bytes;
            if held.old_rows > 0 {
                changed.push((key, no, held));
            }
        }
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        for (key, no, held) in changed {
            self.forget(&key, no, cut, &held);
        }
        done.freed = self.write_and_compact().map_err(StorageError::Failed)?;
        Ok(done)
    }

    /// Removes stream `no`'s events stamped before `cut`, and makes its record say what is left.
    fn forget(&mut self, key: &str, no: u32, cut: i64, held: &Held) {
        let end = if cut == i64::MAX {
            format!("{:08x}", no.saturating_add(1))
        } else {
            event_key(no, cut, 0)
        };
        self.events.delete_range(&event_key(no, 0, 0), &end);
        let left = self.held_of(key).saturating_sub(held.old_rows);
        self.held.insert(key.to_owned(), left);
        if let Some(s) = self.streams.get_mut(key) {
            s.bytes = (s.bytes - held.old_counted).max(0);
            if let Some(first) = held.first_kept {
                s.first_ms = Some(first);
            } else {
                s.bytes = 0;
                s.first_ms = None;
                s.last_ms = None;
                s.last_ingested_ms = None;
            }
        }
    }

    fn reset(&mut self, dry_run: bool) -> Result<Emptied, String> {
        let mut done = Emptied::default();
        for (name, _) in self.groups.iter() {
            let held = self.group_held(name, 0).map_err(|e| e.to_string())?;
            done.items += held.rows;
            done.bytes += held.bytes;
        }
        if dry_run {
            return Ok(done);
        }
        let groups: Vec<String> = self.groups.iter().map(|(k, _)| k.to_owned()).collect();
        for k in &groups {
            self.groups.remove(k);
        }
        let streams: Vec<String> = self.streams.iter().map(|(k, _)| k.to_owned()).collect();
        for k in &streams {
            self.streams.remove(k);
        }
        let docs: Vec<String> = self.docs.iter().map(|(k, _)| k.to_owned()).collect();
        for k in &docs {
            self.docs.remove(k);
        }
        let held: Vec<String> = self.held.iter().map(|(k, _)| k.to_owned()).collect();
        for k in &held {
            self.held.remove(k);
        }
        // Every row key is lower-case hex.
        self.events.delete_range("", "g");
        self.swept.clear();
        self.defaulted.clear();
        done.freed = self.write_and_compact()?;
        Ok(done)
    }

    /// Writes what changed and compacts; answers the bytes the disk got back.
    fn write_and_compact(&mut self) -> Result<u64, String> {
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.flush().map_err(|e| e.to_string())?);
        self.compact_file()
    }

    fn compact_file(&mut self) -> Result<u64, String> {
        match self.db.as_mut() {
            Some(db) => db.compact().map(|c| c.freed()).map_err(|e| e.to_string()),
            None => Ok(0),
        }
    }
}

impl Logs {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let actor = self.actors.actor(region).ok()?;
        let (reply, answer) = oneshot::channel();
        let asked = Asked {
            region: region.to_owned(),
            ask,
            reply,
        };
        actor.call(Ask::Data(Box::new(asked))).await.ok()?;
        answer.await.ok()
    }

    fn regions(&self) -> Vec<String> {
        self.actors.keys_on_disk()
    }
}

impl Storage for Logs {
    fn names(&self) -> Names {
        Names {
            one: "log group",
            many: "log groups",
            units: "events",
            prunes: &["before"],
        }
    }

    fn usage(&self) -> BoxFuture<'_, Result<Usage, String>> {
        Box::pin(async move {
            let mut usage = Usage::default();
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Usage).await {
                    Some(DataReply::Usage(Ok((resources, file)))) => {
                        usage.resources.extend(resources);
                        usage.files.push(file);
                    }
                    Some(DataReply::Usage(Err(e))) => {
                        return Err(format!("CloudWatch Logs in {region}: {e}"));
                    }
                    _ => return Err(format!("CloudWatch Logs in {region} did not answer")),
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
                    "CloudWatch Logs in {region} did not answer"
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
                    Some(DataReply::Emptied(Err(e))) => {
                        return Err(format!("CloudWatch Logs in {region}: {e}"));
                    }
                    _ => return Err(format!("CloudWatch Logs in {region} did not answer")),
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
                    _ => return Err(format!("CloudWatch Logs in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
