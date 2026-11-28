//! Kinesis's data for a person to see and remove (`doze-aws data`): each stream's records, emptied whole or of those that arrived
//! before a time, and the database compacted after, so the disk gets the space back. A stream whose retention is above the default
//! 24 hours says so, with the prune that keeps just the last day.
//!
//! Emptying keeps the stream, its shards and their numbering: each shard's last sequence number is in the stream's record, not in
//! its rows, so the next record takes a number above every one given before. What a consumer sees is what it would see had the
//! records passed their retention: an iterator it holds stays valid and reads on from the first record put after; `TRIM_HORIZON`
//! starts at the first record kept; `AT_SEQUENCE_NUMBER` of a removed record reads on from the next record kept.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::{Db, StoreError};
use tokio::sync::oneshot;

use crate::Kinesis;
use crate::core::{Ask, Core, RECORDS};
use crate::model::{Stream, ms_seq, row_key};

/// The retention a stream has unless someone raises it.
const DEFAULT_HOURS: i64 = 24;

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

/// The rows of a stream: every key starts with its number.
fn bounds(no: u32) -> (String, String) {
    (format!("{no:08x}"), format!("{:08x}", no.saturating_add(1)))
}

/// The sequence number in a row key (stream, shard, then 32 hex digits).
fn seq_of(key: &str) -> u128 {
    key.get(16..)
        .and_then(|h| u128::from_str_radix(h, 16).ok())
        .unwrap_or(0)
}

/// Rows and bytes, all of them and those below a sequence number.
#[derive(Clone, Copy, Debug, Default)]
struct Held {
    rows: u64,
    bytes: u64,
    old_rows: u64,
    old_bytes: u64,
}

impl Core {
    /// Answers a data question. Deletes are handed to the writer before the file is compacted, and compaction waits for them.
    pub(crate) async fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region).map_err(|e| e.to_string())),
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune).await),
            DataAsk::Reset { dry_run } => {
                DataReply::Emptied(self.reset(dry_run).await.map_err(StorageError::Failed))
            }
            DataAsk::Compact => DataReply::Compacted(self.compact_file().await),
        }
    }

    /// What a stream's rows hold, and how much of it is below sequence number `cut`.
    fn held(&self, s: &Stream, cut: u128) -> Result<Held, StoreError> {
        let mut held = Held::default();
        let Some(db) = &self.db else {
            return Ok(held);
        };
        let (from, to) = bounds(s.no);
        db.scan(RECORDS, &from, |k, v| {
            if k >= to.as_str() {
                return false;
            }
            let size = (k.len() + v.len()) as u64;
            held.rows += 1;
            held.bytes += size;
            if seq_of(k) < cut {
                held.old_rows += 1;
                held.old_bytes += size;
            }
            true
        })?;
        Ok(held)
    }

    fn usage(&self, region: &str) -> Result<(Vec<Resource>, File), StoreError> {
        let day_ago = ms_seq(self.now_ms() - DEFAULT_HOURS * 3_600_000);
        let mut out = Vec::new();
        for (name, s) in self.streams.iter() {
            let held = self.held(s, day_ago)?;
            let mut notes = Vec::new();
            if s.retention_hours > DEFAULT_HOURS && held.rows > 0 {
                notes.push(Note {
                    bytes: held.old_bytes,
                    text: format!(
                        "retention raised to {} hours (the default is {DEFAULT_HOURS}): older records are kept",
                        s.retention_hours
                    ),
                    prune: Some(Prune {
                        before_ms: Some(self.now_ms() - DEFAULT_HOURS * 3_600_000),
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

    async fn empty(&mut self, name: &str, prune: &Prune) -> Result<Emptied, StorageError> {
        if prune.prefix.is_some() || prune.old_versions || prune.unused {
            return Err(StorageError::Unsupported(
                "a stream can be emptied whole, or of the records that arrived before a time (--older-than)".to_owned(),
            ));
        }
        let s = self
            .streams
            .get(name)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        // A sequence number carries the millisecond its record arrived in, so a time is a sequence number.
        let cut = prune.before_ms.map_or(u128::MAX, ms_seq);
        let held = self
            .held(&s, cut)
            .map_err(|e| StorageError::Failed(e.to_string()))?;
        let mut done = Emptied {
            items: held.old_rows,
            bytes: held.old_bytes,
            freed: 0,
        };
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        if prune.before_ms.is_some() {
            for shard in &s.shards {
                self.rows
                    .delete_range(&row_key(s.no, shard.id, 0), &row_key(s.no, shard.id, cut));
            }
        } else {
            let (from, to) = bounds(s.no);
            self.rows.delete_range(&from, &to);
        }
        done.freed = self
            .write_and_compact()
            .await
            .map_err(StorageError::Failed)?;
        Ok(done)
    }

    async fn reset(&mut self, dry_run: bool) -> Result<Emptied, String> {
        let mut done = Emptied::default();
        for (_, s) in self.streams.iter() {
            let held = self.held(s, 0).map_err(|e| e.to_string())?;
            done.items += held.rows;
            done.bytes += held.bytes;
        }
        if dry_run {
            return Ok(done);
        }
        let names: Vec<String> = self.streams.iter().map(|(n, _)| n.to_owned()).collect();
        for name in names {
            self.streams.remove(&name);
        }
        let settings: Vec<String> = self.account.iter().map(|(k, _)| k.to_owned()).collect();
        for key in settings {
            self.account.remove(&key);
        }
        // Every row key is lower-case hex.
        self.rows.delete_range("", "g");
        self.trimmed.clear();
        done.freed = self.write_and_compact().await?;
        Ok(done)
    }

    /// Writes what changed and compacts; answers the bytes the disk got back.
    async fn write_and_compact(&mut self) -> Result<u64, String> {
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.flush().map_err(|e| e.to_string())?);
        self.compact_file().await
    }

    /// Compacts on a blocking thread ([`doze_store::Db::compact_off_loop`]): the region's calls wait, the rest of the server does not.
    async fn compact_file(&mut self) -> Result<u64, String> {
        match self.db.as_mut() {
            Some(db) => db
                .compact_off_loop()
                .await
                .map(|c| c.freed())
                .map_err(|e| e.to_string()),
            None => Ok(0),
        }
    }
}

impl Kinesis {
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

impl Storage for Kinesis {
    fn names(&self) -> Names {
        Names {
            one: "stream",
            many: "streams",
            units: "records",
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
                        return Err(format!("Kinesis in {region}: {e}"));
                    }
                    _ => return Err(format!("Kinesis in {region} did not answer")),
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
                    "Kinesis in {region} did not answer"
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
                        return Err(format!("Kinesis in {region}: {e}"));
                    }
                    _ => return Err(format!("Kinesis in {region} did not answer")),
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
                    _ => return Err(format!("Kinesis in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
