//! DynamoDB's data for a person to see and remove (`doze-aws data`): each table's items, the entries its indexes and time to live
//! keep for them, its stream's records and its point-in-time recovery log, and each on-demand backup (a resource of its own,
//! `<table>/backup/<id>`, since it outlives its table), emptied whole, and the database compacted after, so the disk gets the space back. A
//! table's stream records are counted with it and pointed at: they stay for 24 hours after every write whether or not anything
//! reads them.
//!
//! An item carries no time it was written (a `PutItem` today and one a year ago look alike on disk), so a table is emptied whole
//! only: an older-than prune is refused rather than guessed at.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use tokio::sync::oneshot;

use crate::Dynamodb;
use crate::core::{Core, Request};
use crate::keys::{self, Space};
use crate::meta::Table;

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

/// Above every stream row's key (16 bytes of stream id and 8 of sequence number).
const STREAM_END: [u8; 25] = [0xFF; 25];

/// What one table holds: its items, its index and time-to-live entries, its stream's records, its recovery log, each as (rows,
/// bytes).
#[derive(Clone, Copy, Debug, Default)]
struct Held {
    items: (u64, u64),
    index: (u64, u64),
    stream: (u64, u64),
    log: (u64, u64),
}

impl Held {
    const fn bytes(&self) -> u64 {
        self.items.1 + self.index.1 + self.stream.1 + self.log.1
    }
}

/// Above every key of the backup and recovery-log spaces (16 bytes of id, then more).
const ALL_END: [u8; 17] = [0xFF; 17];

/// A backup's name as `doze-aws data` shows it: the resource part of its ARN, `<table>/backup/<id>`.
fn backup_name(b: &crate::extras::Backup) -> &str {
    b.arn
        .rsplit_once(":table/")
        .map_or(b.arn.as_str(), |(_, r)| r)
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

    fn held(&self, t: &Table) -> Result<Held, doze_store::StoreError> {
        let end = keys::prefix_end(&t.id);
        let stream = match &t.stream {
            Some(s) => self
                .disk
                .measure(Space::Stream, &s.id, &keys::prefix_end(&s.id))?,
            None => (0, 0),
        };
        Ok(Held {
            items: self.disk.measure(Space::Items, &t.id, &end)?,
            index: self.disk.measure(Space::Index, &t.id, &end)?,
            stream,
            log: self.log_size(&t.id)?,
        })
    }

    /// Each backup, as a resource of its own: it outlives its table.
    fn backup_usage(&self, region: &str) -> Result<Vec<Resource>, doze_store::StoreError> {
        let mut out = Vec::new();
        for (_, e) in self.extras.with_prefix("backup/") {
            let crate::extras::Extra::Backup(b) = e else {
                continue;
            };
            let (rows, bytes) =
                self.disk
                    .measure(Space::Backup, &b.id, &keys::prefix_end(&b.id))?;
            out.push(Resource {
                region: region.to_owned(),
                name: backup_name(b).to_owned(),
                bytes,
                items: rows,
                notes: vec![Note {
                    bytes,
                    text: format!(
                        "an on-demand backup ({}) of table {}, kept until deleted",
                        b.name, b.table.name
                    ),
                    prune: None,
                }],
            });
        }
        Ok(out)
    }

    /// Deletes a backup by the name `doze-aws data` shows.
    fn empty_backup(&mut self, name: &str, dry_run: bool) -> Result<Emptied, StorageError> {
        let found = self
            .extras
            .with_prefix("backup/")
            .find_map(|(k, e)| match e {
                crate::extras::Extra::Backup(b) if backup_name(b) == name => {
                    Some((k.to_owned(), b.id))
                }
                _ => None,
            });
        let (key, id) = found.ok_or(StorageError::NotFound)?;
        let (items, bytes) = self
            .disk
            .measure(Space::Backup, &id, &keys::prefix_end(&id))
            .map_err(|e| StorageError::Failed(e.to_string()))?;
        let mut done = Emptied {
            items,
            bytes,
            freed: 0,
        };
        if dry_run {
            return Ok(done);
        }
        self.extras.remove(&key);
        self.pending
            .ranges
            .push((Space::Backup, id.to_vec(), keys::prefix_end(&id)));
        done.freed = self.write_and_compact().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn usage(&self, region: &str) -> Result<(Vec<Resource>, File), doze_store::StoreError> {
        let mut out = Vec::new();
        for (name, t) in self.tables.iter() {
            let held = self.held(t)?;
            let mut notes = Vec::new();
            if held.stream.0 > 0 {
                notes.push(Note {
                    bytes: held.stream.1,
                    text: format!(
                        "in {} stream records, kept 24 hours whether or not anything reads them",
                        held.stream.0
                    ),
                    prune: None,
                });
            }
            if held.log.0 > 0 {
                notes.push(Note {
                    bytes: held.log.1,
                    text: format!(
                        "in {} point-in-time recovery log entries, kept a day while recovery is on",
                        held.log.0
                    ),
                    prune: None,
                });
            }
            out.push(Resource {
                region: region.to_owned(),
                name: name.to_owned(),
                bytes: held.bytes(),
                items: held.items.0,
                notes,
            });
        }
        out.extend(self.backup_usage(region)?);
        let space = self.disk.space()?;
        let file = File {
            region: region.to_owned(),
            bytes: space.file,
            reclaimable: space.reclaimable(),
        };
        Ok((out, file))
    }

    fn empty(&mut self, name: &str, prune: &Prune) -> Result<Emptied, StorageError> {
        if prune.before_ms.is_some() || prune.prefix.is_some() || prune.old_versions || prune.unused
        {
            return Err(StorageError::Unsupported(
                "a table can be emptied whole only: its items carry no time they were written"
                    .to_owned(),
            ));
        }
        if name.contains("/backup/") {
            return self.empty_backup(name, prune.dry_run);
        }
        let t = self
            .tables
            .get(name)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        let held = self
            .held(&t)
            .map_err(|e| StorageError::Failed(e.to_string()))?;
        let mut done = Emptied {
            items: held.items.0,
            bytes: held.bytes(),
            freed: 0,
        };
        if prune.dry_run || held.bytes() == 0 {
            return Ok(done);
        }
        if let Some(s) = &t.stream {
            // The stream's numbering goes on from where it was: a consumer's position stays below every record still to come.
            let last = self
                .last_seq(&s.id)
                .map_err(|e| StorageError::Failed(e.message))?;
            if let Some(live) = self.tables.get_mut(name).and_then(|t| t.stream.as_mut()) {
                live.last_seq = last;
            }
            self.pending
                .ranges
                .push((Space::Stream, s.id.to_vec(), keys::prefix_end(&s.id)));
        }
        for space in [Space::Items, Space::Index, Space::Pitr] {
            self.pending
                .ranges
                .push((space, t.id.to_vec(), keys::prefix_end(&t.id)));
        }
        done.freed = self.write_and_compact().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn reset(&mut self, dry_run: bool) -> Result<Emptied, String> {
        let mut done = Emptied::default();
        for (_, t) in self.tables.iter() {
            let held = self.held(t).map_err(|e| e.to_string())?;
            done.items += held.items.0;
            done.bytes += held.items.1 + held.index.1;
        }
        // Every stream row, a table's or one left by a stream switched off before such rows were removed with it.
        done.bytes += self
            .disk
            .measure(Space::Stream, &[], &STREAM_END)
            .map_err(|e| e.to_string())?
            .1;
        // Every backup and every recovery log entry, a table's or one left by a table since deleted.
        for space in [Space::Backup, Space::Pitr] {
            let (rows, bytes) = self
                .disk
                .measure(space, &[], &ALL_END)
                .map_err(|e| e.to_string())?;
            done.bytes += bytes;
            if space == Space::Backup {
                done.items += rows;
            }
        }
        if dry_run {
            return Ok(done);
        }
        for space in [Space::Backup, Space::Pitr] {
            self.pending
                .ranges
                .push((space, Vec::new(), ALL_END.to_vec()));
        }
        let extras: Vec<String> = self.extras.iter().map(|(k, _)| k.to_owned()).collect();
        for k in extras {
            self.extras.remove(&k);
        }
        let names: Vec<String> = self.tables.iter().map(|(n, _)| n.to_owned()).collect();
        for name in names {
            self.drop_table(&name);
        }
        let left: Vec<String> = self.insights.iter().map(|(k, _)| k.to_owned()).collect();
        for k in left {
            self.insights.remove(&k);
        }
        self.pending
            .ranges
            .push((Space::Stream, Vec::new(), STREAM_END.to_vec()));
        self.seqs.clear();
        self.tokens.clear();
        done.freed = self.write_and_compact()?;
        Ok(done)
    }

    /// Writes what changed and compacts; answers the bytes the disk got back.
    fn write_and_compact(&mut self) -> Result<u64, String> {
        let mut writes = std::mem::take(&mut self.pending);
        writes
            .meta
            .extend(self.stage_meta().map_err(|e| e.to_string())?);
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.disk.submit(writes));
        self.compact_file()
    }

    fn compact_file(&mut self) -> Result<u64, String> {
        self.disk
            .compact()
            .map(|c| c.freed())
            .map_err(|e| e.to_string())
    }
}

impl Dynamodb {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let actor = self.actors.actor(region).ok()?;
        let (reply, answer) = oneshot::channel();
        let asked = Asked {
            region: region.to_owned(),
            ask,
            reply,
        };
        let done = actor.call(Request::Data(Box::new(asked))).await.ok()?;
        // An emptied table may have held the item the time-to-live timer was sleeping to.
        self.expiry.arm(region, &actor, done.next_due);
        answer.await.ok()
    }

    fn regions(&self) -> Vec<String> {
        self.actors.keys_on_disk()
    }
}

impl Storage for Dynamodb {
    fn names(&self) -> Names {
        Names {
            one: "table",
            many: "tables",
            units: "items",
            prunes: &[],
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
                        return Err(format!("DynamoDB in {region}: {e}"));
                    }
                    _ => return Err(format!("DynamoDB in {region} did not answer")),
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
                    "DynamoDB in {region} did not answer"
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
                        return Err(format!("DynamoDB in {region}: {e}"));
                    }
                    _ => return Err(format!("DynamoDB in {region} did not answer")),
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
                    _ => return Err(format!("DynamoDB in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
