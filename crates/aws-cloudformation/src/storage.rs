//! CloudFormation's data for a person to see and remove (`doze-aws data`). A resource is a stack name; its bytes are the rows of
//! its stacks (template as given and as processed, parameters, resources), their events and change sets. What grows unnoticed is
//! deleted stacks: AWS lists a `DELETE_COMPLETE` stack for 90 days; here nothing ages one out, so each create-and-delete leaves a
//! stack with its templates and events behind. They are noted, and emptying a name removes its deleted stacks (all, or those
//! deleted before a time) with their events and change sets. A live stack, its events and what it provisioned are never touched by
//! emptying. A reset removes every stack record; it does not delete what the stacks provisioned in other services (reset those, or
//! everything, to start again).

use std::collections::BTreeMap;

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::record;
use tokio::sync::oneshot;

use crate::Cloudformation;
use crate::core::{Core, Request, VERSION};
use crate::model::Stack;

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

fn encoded<T: serde::Serialize>(key: &str, value: &T) -> u64 {
    (key.len() + record::encode("record", VERSION, value).map_or(0, |b| b.len())) as u64
}

/// One stack's rows: its events and the bytes of everything it owns.
#[derive(Default, Clone, Copy)]
struct Size {
    events: u64,
    bytes: u64,
}

/// What a stack name holds: live and deleted.
#[derive(Default)]
struct Tally {
    live: Size,
    deleted: Size,
    deleted_count: u64,
}

fn is_deleted(s: &Stack) -> bool {
    s.status == "DELETE_COMPLETE"
}

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) async fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region), self.file(region)),
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune).await),
            DataAsk::Reset { dry_run } => DataReply::Emptied(Ok(self.reset(dry_run).await)),
            DataAsk::Compact => DataReply::Compacted(self.compact().await),
        }
    }

    /// The size of each stack, by stack id.
    fn sizes(&self) -> BTreeMap<String, Size> {
        let mut by: BTreeMap<String, Size> = self
            .stacks
            .iter()
            .map(|(k, s)| {
                (
                    s.id.clone(),
                    Size {
                        events: 0,
                        bytes: encoded(k, s),
                    },
                )
            })
            .collect();
        for (k, e) in self.events.iter() {
            let size = by.entry(e.stack_id.clone()).or_default();
            size.events += 1;
            size.bytes += encoded(k, e);
        }
        for (k, c) in self.change_sets.iter() {
            by.entry(c.stack_id.clone()).or_default().bytes += encoded(k, c);
        }
        for (k, d) in self.detections.iter() {
            by.entry(d.stack_id.clone()).or_default().bytes += encoded(k, d);
        }
        for (k, d) in self.drifts.iter() {
            by.entry(d.stack_id.clone()).or_default().bytes += encoded(k, d);
        }
        for (k, o) in self.operations.iter() {
            by.entry(o.stack_id.clone()).or_default().bytes += encoded(k, o);
        }
        by
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        let sizes = self.sizes();
        let mut by: BTreeMap<String, Tally> = BTreeMap::new();
        for (_, s) in self.stacks.iter() {
            let size = sizes.get(&s.id).copied().unwrap_or_default();
            let t = by.entry(s.name.clone()).or_default();
            let slot = if is_deleted(s) {
                t.deleted_count += 1;
                &mut t.deleted
            } else {
                &mut t.live
            };
            slot.events += size.events;
            slot.bytes += size.bytes;
        }
        by.into_iter()
            .map(|(name, t)| {
                let mut notes = Vec::new();
                if t.deleted_count > 0 {
                    notes.push(Note {
                        bytes: t.deleted.bytes,
                        text: format!(
                            "in {} deleted stacks, kept with their templates and events",
                            t.deleted_count
                        ),
                        prune: Some(Prune::default()),
                    });
                }
                Resource {
                    region: region.to_owned(),
                    name,
                    bytes: t.live.bytes + t.deleted.bytes,
                    items: t.live.events + t.deleted.events,
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

    async fn empty(&mut self, name: &str, prune: &Prune) -> Result<Emptied, StorageError> {
        if prune.prefix.is_some() || prune.old_versions || prune.unused {
            return Err(StorageError::Unsupported(
                "a stack name can be emptied of its deleted stacks, all or those deleted before a time (--older-than); a live stack is changed through CloudFormation"
                    .to_owned(),
            ));
        }
        if !self.stacks.iter().any(|(_, s)| s.name == name) {
            return Err(StorageError::NotFound);
        }
        let doomed: Vec<(String, String)> = self
            .stacks
            .iter()
            .filter(|(_, s)| {
                s.name == name
                    && is_deleted(s)
                    && prune
                        .before_ms
                        .is_none_or(|b| s.deleted_ms.unwrap_or(s.created_ms) < b)
            })
            .map(|(k, s)| (k.to_owned(), s.id.clone()))
            .collect();
        let sizes = self.sizes();
        let mut done = Emptied::default();
        for (_, id) in &doomed {
            let size = sizes.get(id).copied().unwrap_or_default();
            done.items += size.events;
            done.bytes += size.bytes;
        }
        if prune.dry_run || doomed.is_empty() {
            return Ok(done);
        }
        for (key, id) in &doomed {
            self.forget_stack(key, id);
        }
        done.freed = self.settle_data().await.map_err(StorageError::Failed)?;
        Ok(done)
    }

    /// Removes a stack's record, events and change sets.
    fn forget_stack(&mut self, key: &str, id: &str) {
        self.stacks.remove(key);
        let events: Vec<String> = self
            .events
            .iter()
            .filter(|(_, e)| e.stack_id == id)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in events {
            self.events.remove(&k);
        }
        let sets: Vec<String> = self
            .change_sets
            .iter()
            .filter(|(_, c)| c.stack_id == id)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in sets {
            self.change_sets.remove(&k);
        }
        self.forget_drift(id);
        let ops: Vec<String> = self
            .operations
            .iter()
            .filter(|(_, o)| o.stack_id == id)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in ops {
            self.operations.remove(&k);
        }
    }

    async fn reset(&mut self, dry_run: bool) -> Emptied {
        let mut done = Emptied::default();
        for size in self.sizes().values() {
            done.items += size.events;
            done.bytes += size.bytes;
        }
        if dry_run {
            return done;
        }
        clear(&mut self.stacks);
        clear(&mut self.events);
        clear(&mut self.change_sets);
        clear(&mut self.detections);
        clear(&mut self.drifts);
        clear(&mut self.operations);
        self.outbox.clear();
        self.started.clear();
        self.checks.clear();
        // WHY: a failed write is the writer's to report on the next call; the counts say what was asked.
        done.freed = self.settle_data().await.unwrap_or(0);
        done
    }

    /// Writes what changed, waits for it to be on disk, and compacts; answers the bytes the disk got back.
    async fn settle_data(&mut self) -> Result<u64, String> {
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.flush().map_err(|e| e.to_string())?);
        self.compact().await
    }

    async fn compact(&mut self) -> Result<u64, String> {
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

/// Removes every record of a table.
fn clear<T: serde::Serialize + serde::de::DeserializeOwned>(
    records: &mut doze_store::records::Records<T>,
) {
    let keys: Vec<String> = records.iter().map(|(k, _)| k.to_owned()).collect();
    for key in keys {
        records.remove(&key);
    }
}

impl Cloudformation {
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

impl Storage for Cloudformation {
    fn names(&self) -> Names {
        Names {
            one: "stack",
            many: "stacks",
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
                    _ => return Err(format!("CloudFormation in {region} did not answer")),
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
                    "CloudFormation in {region} did not answer"
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
                    _ => return Err(format!("CloudFormation in {region} did not answer")),
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
                    _ => return Err(format!("CloudFormation in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
