//! Step Functions' data for a person to see and remove (`doze-aws data`). A resource is a state machine; its bytes are its
//! executions and their history events (and Map Runs). Emptying removes executions that have ended, whole or those that ended
//! before a time; a running execution is never removed (stop it first), and the machine stays. Executions of a machine that was
//! deleted stay too (AWS keeps them 90 days; here nothing ages them out): they are listed under the machine's name, with a note.

use std::collections::BTreeMap;

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::record;
use tokio::sync::oneshot;

use crate::Sfn;
use crate::core::{Asked, Core, HISTORY, Request, VERSION};
use crate::model::{Execution, Run};

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

/// One machine's executions, as counted.
#[derive(Default)]
struct Tally {
    running: (u64, u64),
    ended: (u64, u64),
    deleted: bool,
}

/// The name of the machine an execution ran.
fn machine_of(e: &Execution) -> String {
    crate::arn::target(&e.machine_arn)
        .map_or_else(|| e.machine_arn.clone(), |(_, t)| t.machine().to_owned())
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

    /// The bytes each execution's history events hold, by execution ARN.
    fn history_bytes(&self) -> BTreeMap<String, u64> {
        let mut by = BTreeMap::new();
        if let Some(db) = &self.db {
            // WHY: a failed read reports what was counted; the next usage reads again.
            drop(db.scan(HISTORY, "", |key, value| {
                let arn = key.rsplit_once('/').map_or(key, |(a, _)| a);
                *by.entry(arn.to_owned()).or_insert(0) += (key.len() + value.len()) as u64;
                true
            }));
        }
        by
    }

    /// An execution's bytes: its row, its Map Runs' rows, and its history events.
    fn bytes_of(&self, e: &Execution, history: &BTreeMap<String, u64>) -> u64 {
        let row = record::encode("execution", VERSION, e).map_or(0, |b| b.len() as u64);
        let runs: u64 = self
            .map_runs
            .iter()
            .filter(|(_, m)| m.execution == e.arn)
            .map(|(_, m)| record::encode("map run", VERSION, m).map_or(0, |b| b.len() as u64))
            .sum();
        row + runs + history.get(&e.arn).copied().unwrap_or(0)
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        let history = self.history_bytes();
        let mut by: BTreeMap<String, Tally> = self
            .machines
            .iter()
            .map(|(_, m)| (m.name.clone(), Tally::default()))
            .collect();
        for (_, e) in self.executions.iter() {
            let t = by.entry(machine_of(e)).or_insert_with(|| Tally {
                deleted: true,
                ..Tally::default()
            });
            let slot = if e.status == Run::Running {
                &mut t.running
            } else {
                &mut t.ended
            };
            slot.0 += 1;
            slot.1 += self.bytes_of(e, &history);
        }
        by.into_iter()
            .map(|(name, t)| Resource {
                region: region.to_owned(),
                name,
                bytes: t.running.1 + t.ended.1,
                items: t.running.0 + t.ended.0,
                notes: notes(&t),
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
                "a state machine can be emptied of its ended executions, whole or those that ended before a time (--older-than)"
                    .to_owned(),
            ));
        }
        let known = self.machines.iter().any(|(_, m)| m.name == name)
            || self.executions.iter().any(|(_, e)| machine_of(e) == name);
        if !known {
            return Err(StorageError::NotFound);
        }
        let goes = |e: &Execution| {
            e.status != Run::Running
                && machine_of(e) == name
                && prune
                    .before_ms
                    .is_none_or(|before| e.stop_ms.unwrap_or(e.start_ms) < before)
        };
        let doomed: Vec<String> = self
            .executions
            .iter()
            .filter(|(_, e)| goes(e))
            .map(|(k, _)| k.to_owned())
            .collect();
        let history = self.history_bytes();
        let mut done = Emptied::default();
        for arn in &doomed {
            if let Some(e) = self.executions.get(arn) {
                done.items += 1;
                done.bytes += self.bytes_of(e, &history);
            }
        }
        if prune.dry_run || doomed.is_empty() {
            return Ok(done);
        }
        for arn in &doomed {
            self.forget(arn);
        }
        done.freed = self.settle().map_err(StorageError::Failed)?;
        Ok(done)
    }

    /// Removes an execution, its Map Runs and its history.
    fn forget(&mut self, arn: &str) {
        self.remove_execution(arn);
        let runs: Vec<String> = self
            .map_runs
            .iter()
            .filter(|(_, m)| m.execution == arn)
            .map(|(k, _)| k.to_owned())
            .collect();
        for run in runs {
            self.map_runs.remove(&run);
        }
        self.history
            .delete_range(&format!("{arn}/"), &format!("{arn}0"));
    }

    fn reset(&mut self, dry_run: bool) -> Emptied {
        let history = self.history_bytes();
        let mut done = Emptied::default();
        for (_, e) in self.executions.iter() {
            done.items += 1;
            done.bytes += self.bytes_of(e, &history);
        }
        if dry_run {
            return done;
        }
        clear(&mut self.machines);
        clear(&mut self.activities);
        clear(&mut self.executions);
        clear(&mut self.map_runs);
        self.history.delete_range("", "\u{10ffff}");
        self.parsed.clear();
        self.queue.clear();
        self.spent.clear();
        self.last_caller = None;
        // WHY: a failed write is the writer's to report on the next call; the counts say what was asked.
        done.freed = self.settle().unwrap_or(0);
        done
    }

    /// Writes what changed, waits for it to be on disk, and compacts; answers the bytes the disk got back.
    fn settle(&mut self) -> Result<u64, String> {
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

/// What a machine holds that a person may not know is there.
fn notes(t: &Tally) -> Vec<Note> {
    let mut notes = Vec::new();
    if t.deleted && t.ended.0 > 0 {
        notes.push(Note {
            bytes: t.ended.1,
            text: "the state machine was deleted; its executions are kept until removed".to_owned(),
            prune: Some(Prune::default()),
        });
    } else if t.ended.0 > 0 && t.ended.1 >= t.running.1 {
        notes.push(Note {
            bytes: t.ended.1,
            text: format!(
                "in {} ended executions and their history: nothing ages them out",
                t.ended.0
            ),
            prune: Some(Prune::default()),
        });
    }
    if t.running.0 > 0 {
        notes.push(Note {
            bytes: t.running.1,
            text: format!(
                "in {} running executions: emptying never removes them (stop them first)",
                t.running.0
            ),
            prune: None,
        });
    }
    notes
}

impl Sfn {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let actor = self.shared.actors.actor(region).ok()?;
        let (reply, answer) = oneshot::channel();
        let call = DataCall {
            region: region.to_owned(),
            ask,
            reply,
        };
        actor
            .call(Asked {
                config: std::sync::Arc::clone(&self.shared.config),
                req: Request::Data(Box::new(call)),
            })
            .await
            .ok()?;
        answer.await.ok()
    }

    fn regions(&self) -> Vec<String> {
        self.shared.actors.keys_on_disk()
    }
}

impl Storage for Sfn {
    fn names(&self) -> Names {
        Names {
            one: "state machine",
            many: "state machines",
            units: "executions",
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
                    _ => return Err(format!("Step Functions in {region} did not answer")),
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
                    "Step Functions in {region} did not answer"
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
                    _ => return Err(format!("Step Functions in {region} did not answer")),
                }
            }
            if !dry_run {
                self.shared.forget_waiters();
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
                    _ => return Err(format!("Step Functions in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
