//! Secrets Manager's data for a person to see and remove (`doze-aws data`). A secret is a definition; what grows is its versions:
//! every `PutSecretValue` and rotation keeps the version before, and one that loses its last staging label is kept as AWS keeps a
//! deprecated version (up to 100 versions a secret). A secret with more than one version is listed with its bytes; its versions
//! with no staging label are noted, and emptying it with `--old-versions` removes them. The versions that carry a label
//! (`AWSCURRENT`, `AWSPREVIOUS`, `AWSPENDING` or one of the person's) always stay. A secret scheduled for deletion is noted: it is
//! kept until its recovery window ends. A reset removes every secret and its rotation settings and replication (a replica is a secret of its own region: it is listed and emptied as one).

use aws_wire::service::BoxFuture;
use aws_wire::storage::{
    Emptied, File, Names, Note, Prune, Resource, Storage, StorageError, Usage,
};
use doze_store::record;
use tokio::sync::oneshot;

use crate::Secrets;
use crate::core::{Core, Request, VERSION};
use crate::model::{Secret, Version};

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

fn row_bytes(key: &str, s: &Secret) -> u64 {
    (key.len() + record::encode("secret", VERSION, s).map_or(0, |b| b.len())) as u64
}

fn version_bytes(id: &str, v: &Version) -> u64 {
    (id.len() + record::encode("secret", VERSION, v).map_or(0, |b| b.len())) as u64
}

/// The versions an `--old-versions` prune removes: those with no staging label.
fn unlabelled(s: &Secret) -> impl Iterator<Item = (&String, &Version)> {
    s.versions.iter().filter(|(_, v)| v.stages.is_empty())
}

fn notes(s: &Secret) -> Vec<Note> {
    let mut notes = Vec::new();
    let old: Vec<u64> = unlabelled(s).map(|(id, v)| version_bytes(id, v)).collect();
    if !old.is_empty() {
        notes.push(Note {
            bytes: old.iter().sum(),
            text: format!("in {} versions with no staging label", old.len()),
            prune: Some(Prune {
                old_versions: true,
                ..Prune::default()
            }),
        });
    }
    if s.deleted_ms.is_some() {
        notes.push(Note {
            bytes: 0,
            text: "scheduled for deletion: kept until its recovery window ends".to_owned(),
            prune: None,
        });
    }
    notes
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

    fn usage(&self, region: &str) -> Vec<Resource> {
        self.secrets
            .iter()
            .filter(|(_, s)| s.versions.len() > 1 || s.deleted_ms.is_some())
            .map(|(key, s)| Resource {
                region: region.to_owned(),
                name: s.name.clone(),
                bytes: row_bytes(key, s),
                items: s.versions.len() as u64,
                notes: notes(s),
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
                "a secret's value is not data to empty: remove its versions with no staging label (--old-versions), or delete the secret"
                    .to_owned(),
            ));
        }
        let key = self
            .secrets
            .iter()
            .find(|(_, s)| s.name == name)
            .map(|(k, _)| k.to_owned())
            .ok_or(StorageError::NotFound)?;
        let mut done = Emptied::default();
        if let Some(s) = self.secrets.get(&key) {
            for (id, v) in unlabelled(s) {
                done.items += 1;
                done.bytes += version_bytes(id, v);
            }
        }
        if prune.dry_run || done.items == 0 {
            return Ok(done);
        }
        if let Some(s) = self.secrets.get_mut(&key) {
            s.versions.retain(|_, v| !v.stages.is_empty());
        }
        done.freed = self.settle_data().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn reset(&mut self, dry_run: bool) -> Emptied {
        let mut done = Emptied::default();
        for (key, s) in self.secrets.iter() {
            done.items += s.versions.len() as u64;
            done.bytes += row_bytes(key, s);
        }
        if dry_run {
            return done;
        }
        let secrets: Vec<String> = self.secrets.iter().map(|(k, _)| k.to_owned()).collect();
        for key in secrets {
            self.secrets.remove(&key);
        }
        let rotations: Vec<String> = self.rotations.iter().map(|(k, _)| k.to_owned()).collect();
        for key in rotations {
            self.rotations.remove(&key);
        }
        let replicated: Vec<String> = self.replication.iter().map(|(k, _)| k.to_owned()).collect();
        for key in replicated {
            self.replication.remove(&key);
        }
        self.forget_due();
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

impl Secrets {
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

impl Storage for Secrets {
    fn names(&self) -> Names {
        Names {
            one: "secret",
            many: "secrets",
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
                    _ => return Err(format!("Secrets Manager in {region} did not answer")),
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
                    "Secrets Manager in {region} did not answer"
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
                    _ => return Err(format!("Secrets Manager in {region} did not answer")),
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
                    _ => return Err(format!("Secrets Manager in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
