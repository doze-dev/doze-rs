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
use crate::model::Run;

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

/// The name of the machine an execution ran, by the machine's ARN.
fn machine_of(machine_arn: &str) -> String {
    crate::arn::target(machine_arn)
        .map_or_else(|| machine_arn.to_owned(), |(_, t)| t.machine().to_owned())
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

    /// An execution's bytes: its row, its Map Runs' rows, and its history events. A finished one's row is as long as its summary
    /// says, unless it is still in memory.
    fn bytes_of(&self, arn: &str, history: &BTreeMap<String, u64>) -> u64 {
        let row = match (self.executions.get(arn), self.index.summary(arn)) {
            (Some(e), _) => record::encode("execution", VERSION, e).map_or(0, |b| b.len() as u64),
            (None, Some(s)) => s.bytes,
            (None, None) => 0,
        };
        let runs: u64 = self
            .map_runs
            .iter()
            .filter(|(_, m)| m.execution == arn)
            .map(|(_, m)| record::encode("map run", VERSION, m).map_or(0, |b| b.len() as u64))
            .sum();
        row + runs + history.get(arn).copied().unwrap_or(0)
    }

    /// Every execution as `(ARN, machine ARN, status, when it stopped or started)`: the running ones from memory, the finished ones
    /// by their summaries.
    fn every_execution(&self) -> Vec<(String, String, Run, i64)> {
        let running = self
            .executions
            .iter()
            .filter(|(_, e)| e.status == Run::Running)
            .map(|(k, e)| (k.to_owned(), e.machine_arn.clone(), e.status, e.start_ms));
        let finished = self.index.summaries().map(|(k, s)| {
            let when = s.stop_ms.unwrap_or(s.start_ms);
            (k.to_owned(), s.machine_arn.clone(), s.status, when)
        });
        running.chain(finished).collect()
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        let history = self.history_bytes();
        let mut by: BTreeMap<String, Tally> = self
            .machines
            .iter()
            .map(|(_, m)| (m.name.clone(), Tally::default()))
            .collect();
        for (arn, machine, status, _) in self.every_execution() {
            let t = by.entry(machine_of(&machine)).or_insert_with(|| Tally {
                deleted: true,
                ..Tally::default()
            });
            let slot = if status == Run::Running {
                &mut t.running
            } else {
                &mut t.ended
            };
            slot.0 += 1;
            slot.1 += self.bytes_of(&arn, &history);
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

    async fn empty(&mut self, name: &str, prune: &Prune) -> Result<Emptied, StorageError> {
        if prune.prefix.is_some() || prune.old_versions || prune.unused {
            return Err(StorageError::Unsupported(
                "a state machine can be emptied of its ended executions, whole or those that ended before a time (--older-than)"
                    .to_owned(),
            ));
        }
        let every = self.every_execution();
        let known = self.machines.iter().any(|(_, m)| m.name == name)
            || every.iter().any(|(_, m, ..)| machine_of(m) == name);
        if !known {
            return Err(StorageError::NotFound);
        }
        let doomed: Vec<String> = every
            .into_iter()
            .filter(|(_, machine, status, when)| {
                *status != Run::Running
                    && machine_of(machine) == name
                    && prune.before_ms.is_none_or(|before| *when < before)
            })
            .map(|(arn, ..)| arn)
            .collect();
        let history = self.history_bytes();
        let mut done = Emptied::default();
        for arn in &doomed {
            done.items += 1;
            done.bytes += self.bytes_of(arn, &history);
        }
        if prune.dry_run || doomed.is_empty() {
            return Ok(done);
        }
        for arn in &doomed {
            self.forget(arn);
        }
        done.freed = self.settle().await.map_err(StorageError::Failed)?;
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

    async fn reset(&mut self, dry_run: bool) -> Emptied {
        let history = self.history_bytes();
        let mut done = Emptied::default();
        let every = self.every_execution();
        for (arn, ..) in &every {
            done.items += 1;
            done.bytes += self.bytes_of(arn, &history);
        }
        if dry_run {
            return done;
        }
        clear(&mut self.machines);
        clear(&mut self.activities);
        for (arn, ..) in &every {
            self.remove_execution(arn);
        }
        self.index.clear();
        clear(&mut self.map_runs);
        self.history.delete_range("", "\u{10ffff}");
        self.parsed.clear();
        self.queue.clear();
        self.spent.clear();
        self.last_caller = None;
        // WHY: a failed write is the writer's to report on the next call; the counts say what was asked.
        done.freed = self.settle().await.unwrap_or(0);
        done
    }

    /// Writes what changed, waits for it to be on disk, and compacts; answers the bytes the disk got back.
    async fn settle(&mut self) -> Result<u64, String> {
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.flush().map_err(|e| e.to_string())?);
        self.compact().await
    }

    /// Compacts on a blocking thread ([`doze_store::Db::compact_off_loop`]): the region's calls wait, the rest of the server does not.
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
