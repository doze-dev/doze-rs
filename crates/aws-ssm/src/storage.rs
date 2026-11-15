//! Parameter Store's data for a person to see and remove (`doze-aws data`). A parameter is a definition, not a container of data;
//! what grows is its history: every `PutParameter --overwrite` keeps the version before (AWS keeps 100). A parameter with old
//! versions is listed with its bytes and a note, and emptying it with `--old-versions` removes the old versions no label points at
//! (a labelled version is a name someone resolves, so it stays). Its value, its newest version and its tags always stay. A reset
//! removes every parameter, document and changed service setting (`doc_storage.rs` holds the documents' part).

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::record;
use tokio::sync::oneshot;

use crate::Ssm;
use crate::core::{Ask, Core, VERSION};
use crate::model::{Parameter, Version};

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

fn row_bytes(name: &str, p: &Parameter) -> u64 {
    (name.len() + record::encode("parameter", VERSION, p).map_or(0, |b| b.len())) as u64
}

fn version_bytes(v: &Version) -> u64 {
    record::encode("parameter", VERSION, v).map_or(0, |b| b.len() as u64)
}

/// The old versions an `--old-versions` prune removes: those no label points at.
fn removable(p: &Parameter) -> impl Iterator<Item = &Version> {
    p.older.iter().filter(|v| v.labels.is_empty())
}

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => {
                let mut all = self.usage(region);
                all.extend(self.document_usage(region));
                DataReply::Usage(all, self.file(region))
            }
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune)),
            DataAsk::Reset { dry_run } => DataReply::Emptied(Ok(self.reset(dry_run))),
            DataAsk::Compact => DataReply::Compacted(self.compact()),
        }
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        self.params
            .iter()
            .filter(|(_, p)| !p.older.is_empty())
            .map(|(name, p)| {
                let old: Vec<&Version> = removable(p).collect();
                let mut notes = Vec::new();
                if !old.is_empty() {
                    notes.push(Note {
                        bytes: old.iter().map(|v| version_bytes(v)).sum(),
                        text: format!(
                            "in {} no label points at",
                            crate::storage::plural(old.len(), "old version")
                        ),
                        prune: Some(Prune {
                            old_versions: true,
                            ..Prune::default()
                        }),
                    });
                }
                Resource {
                    region: region.to_owned(),
                    name: name.to_owned(),
                    bytes: row_bytes(name, p),
                    items: p.older.len() as u64 + 1,
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
        if !prune.old_versions
            || prune.prefix.is_some()
            || prune.unused
            || prune.before_ms.is_some()
        {
            return Err(StorageError::Unsupported(
                "a parameter's value is not data to empty: remove its old versions (--old-versions), or delete the parameter"
                    .to_owned(),
            ));
        }
        if let Some(doc) = name.strip_prefix(crate::doc_storage::PREFIX)
            && self.params.get(name).is_none()
        {
            let mut done = self.empty_document(doc, prune)?;
            if !prune.dry_run && done.items > 0 {
                done.freed = self.settle_data().map_err(StorageError::Failed)?;
            }
            return Ok(done);
        }
        let p = self.params.get(name).ok_or(StorageError::NotFound)?;
        let mut done = Emptied::default();
        for v in removable(p) {
            done.items += 1;
            done.bytes += version_bytes(v);
        }
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        if let Some(p) = self.params.get_mut(name) {
            p.older.retain(|v| !v.labels.is_empty());
        }
        done.freed = self.settle_data().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn reset(&mut self, dry_run: bool) -> Emptied {
        let mut done = Emptied::default();
        for (name, p) in self.params.iter() {
            done.items += p.older.len() as u64 + 1;
            done.bytes += row_bytes(name, p);
        }
        self.reset_documents(dry_run, &mut done);
        if dry_run {
            return done;
        }
        let names: Vec<String> = self.params.iter().map(|(k, _)| k.to_owned()).collect();
        for name in names {
            self.params.remove(&name);
        }
        self.forget_expiry();
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

impl Ssm {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let actor = self.actors.actor(region).ok()?;
        let (reply, answer) = oneshot::channel();
        let call = DataCall {
            region: region.to_owned(),
            ask,
            reply,
        };
        actor.call(Ask::Data(Box::new(call))).await.ok()?;
        answer.await.ok()
    }

    fn regions(&self) -> Vec<String> {
        self.actors.keys_on_disk()
    }
}

impl Storage for Ssm {
    fn names(&self) -> Names {
        Names {
            one: "parameter",
            many: "parameters",
            units: "versions",
            prunes: &["old-versions"],
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
                    _ => return Err(format!("Parameter Store in {region} did not answer")),
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
                    "Parameter Store in {region} did not answer"
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
                    _ => return Err(format!("Parameter Store in {region} did not answer")),
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
                    _ => return Err(format!("Parameter Store in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}

/// `1 old version`, `3 old versions`.
pub(crate) fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}
