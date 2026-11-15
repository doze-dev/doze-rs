//! SQS's data for a person to see and remove (`doze-aws data`): each queue's messages, emptied whole or older than a time, and the
//! database compacted after, so the disk gets the space back. A dead-letter queue that holds messages says so: nothing reads it
//! unless someone redrives it, so it is the queue most likely to grow unnoticed.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::record;
use tokio::sync::oneshot;

use crate::Sqs;
use crate::core::{Ask, Core};
use crate::model::Message;
use crate::persist::VERSION;

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
pub(crate) struct Asked {
    pub(crate) region: String,
    pub(crate) ask: DataAsk,
    pub(crate) reply: oneshot::Sender<DataReply>,
}

/// The bytes a message takes on disk: its row as written.
fn bytes_of(m: &Message) -> u64 {
    record::encode("message", VERSION, m).map_or(0, |b| b.len() as u64)
}

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region), self.file(region)),
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune)),
            DataAsk::Reset { dry_run } => DataReply::Emptied(Ok(self.reset(dry_run))),
            DataAsk::Compact => DataReply::Compacted(self.compact().map_err(|e| e.to_string())),
        }
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        // A queue that some queue's redrive policy names: its messages wait for a person.
        let dead_letter: std::collections::BTreeSet<String> = self
            .queues
            .values()
            .filter_map(|q| q.redrive())
            .filter_map(|r| r.target_arn.rsplit(':').next().map(str::to_owned))
            .collect();
        self.queues
            .values()
            .map(|q| {
                let bytes = q.messages.iter().map(bytes_of).sum();
                let items = q.messages.len() as u64;
                let mut notes = Vec::new();
                if items > 0 && dead_letter.contains(&q.name) {
                    notes.push(Note {
                        bytes,
                        text:
                            "dead-letter queue: nothing receives from it until someone redrives it"
                                .to_owned(),
                        prune: None,
                    });
                }
                Resource {
                    region: region.to_owned(),
                    name: q.name.clone(),
                    bytes,
                    items,
                    notes,
                }
            })
            .collect()
    }

    fn file(&self, region: &str) -> File {
        let space = self
            .persist
            .as_ref()
            .and_then(|p| p.space().ok())
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
                "a queue can be emptied whole, or of the messages sent before a time (--older-than)".to_owned(),
            ));
        }
        let queue = self.queues.get_mut(name).ok_or(StorageError::NotFound)?;
        let goes = |m: &Message| prune.before_ms.is_none_or(|before| m.sent_ms < before);
        let mut done = Emptied::default();
        for m in queue.messages.iter().filter(|m| goes(m)) {
            done.items += 1;
            done.bytes += bytes_of(m);
        }
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        queue.messages.retain(|m| !goes(m));
        done.freed = self.settle().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn reset(&mut self, dry_run: bool) -> Emptied {
        let mut done = Emptied::default();
        for q in self.queues.values() {
            done.items += q.messages.len() as u64;
            done.bytes += q.messages.iter().map(bytes_of).sum::<u64>();
        }
        if dry_run {
            return done;
        }
        let names: Vec<String> = self.queues.keys().cloned().collect();
        self.removed.extend(names);
        self.queues.clear();
        self.deleted.clear();
        self.tasks.clear();
        // WHY: a failed write is the writer's to report on the next call; the counts say what was asked.
        done.freed = self.settle().unwrap_or(0);
        done
    }

    /// Writes what changed, waits for it to be on disk, and compacts; answers the bytes the disk got back.
    fn settle(&mut self) -> Result<u64, String> {
        // WHY: the writer finishes what it was handed before compaction pauses it, so the deletes are on disk first.
        drop(self.flush().map_err(|e| e.to_string())?);
        self.compact().map_err(|e| e.to_string())
    }

    fn compact(&mut self) -> Result<u64, doze_store::StoreError> {
        match self.persist.as_mut() {
            Some(p) => p.compact().map(|c| c.freed()),
            None => Ok(0),
        }
    }
}

impl Sqs {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let actor = self.actors.actor(region).ok()?;
        let (reply, answer) = oneshot::channel();
        actor
            .call(Ask::Data(Box::new(Asked {
                region: region.to_owned(),
                ask,
                reply,
            })))
            .await
            .ok()?;
        answer.await.ok()
    }

    /// The regions that hold something on disk.
    pub(crate) fn regions(&self) -> Vec<String> {
        self.actors.keys_on_disk()
    }
}

impl Storage for Sqs {
    fn names(&self) -> Names {
        Names {
            one: "queue",
            many: "queues",
            units: "messages",
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
                    _ => return Err(format!("SQS in {region} did not answer")),
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
                    "SQS in {region} did not answer"
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
                    _ => return Err(format!("SQS in {region} did not answer")),
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
                    _ => return Err(format!("SQS in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
