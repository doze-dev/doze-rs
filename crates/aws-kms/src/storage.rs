//! KMS's data for a person to see and remove (`doze-aws data`). KMS keeps definitions only: keys (with their material, sealed),
//! aliases and grants, a few KB, nothing that grows; usage reports its files, emptying is refused with a sentence, and a reset
//! removes every key and alias, the AWS-managed `alias/aws/*` keys too (they are made again when their service first asks, as in a
//! fresh account), and makes the file's sealing key afresh. A reset of KMS alone leaves what other services sealed with its keys
//! (SSM `SecureString`s, secrets) unreadable: `doze-aws data reset` of everything is the way to start again.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{Emptied, File, Names, Prune, Storage, StorageError, Usage};
use tokio::sync::oneshot;

use crate::Kms;
use crate::core::{Ask, Core};

/// What the actor is asked about its data.
#[derive(Debug)]
pub(crate) enum DataAsk {
    Usage,
    Reset { dry_run: bool },
    Compact,
}

/// Its answer.
#[derive(Debug)]
pub(crate) enum DataReply {
    Usage(File),
    Emptied(Result<Emptied, String>),
    Compacted(Result<u64, String>),
}

/// A data question and where its answer goes.
#[derive(Debug)]
pub(crate) struct DataCall {
    pub(crate) region: String,
    pub(crate) ask: DataAsk,
    pub(crate) reply: oneshot::Sender<DataReply>,
}

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) async fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => {
                let space = self
                    .db
                    .as_ref()
                    .and_then(|db| db.space().ok())
                    .unwrap_or_default();
                DataReply::Usage(File {
                    region: region.to_owned(),
                    bytes: space.file,
                    reclaimable: space.reclaimable(),
                })
            }
            DataAsk::Reset { dry_run } => DataReply::Emptied(self.reset(dry_run).await),
            DataAsk::Compact => DataReply::Compacted(self.compact().await),
        }
    }

    async fn reset(&mut self, dry_run: bool) -> Result<Emptied, String> {
        let (items, bytes) = self.count();
        let mut done = Emptied {
            items,
            bytes,
            freed: 0,
        };
        if dry_run {
            return Ok(done);
        }
        self.wipe().map_err(|e| e.to_string())?;
        done.freed = self.compact().await?;
        Ok(done)
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
pub(crate) fn clear<T: serde::Serialize + serde::de::DeserializeOwned>(
    records: &mut doze_store::records::Records<T>,
) {
    let keys: Vec<String> = records.iter().map(|(k, _)| k.to_owned()).collect();
    for key in keys {
        records.remove(&key);
    }
}

/// How many records a table holds, and the bytes of their rows (key and encoded value).
pub(crate) fn tally<T: serde::Serialize + serde::de::DeserializeOwned>(
    records: &doze_store::records::Records<T>,
) -> (u64, u64) {
    records.iter().fold((0, 0), |(n, bytes), (k, v)| {
        let row = doze_store::record::encode("record", 1, v).map_or(0, |b| b.len());
        (n + 1, bytes + (k.len() + row) as u64)
    })
}

impl Kms {
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

impl Storage for Kms {
    fn names(&self) -> Names {
        Names {
            one: "key",
            many: "keys",
            units: "keys",
            prunes: &[],
        }
    }

    fn usage(&self) -> BoxFuture<'_, Result<Usage, String>> {
        Box::pin(async move {
            let mut usage = Usage::default();
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Usage).await {
                    Some(DataReply::Usage(file)) => usage.files.push(file),
                    _ => return Err(format!("KMS in {region} did not answer")),
                }
            }
            Ok(usage)
        })
    }

    fn empty<'a>(
        &'a self,
        _region: &'a str,
        _name: &'a str,
        _prune: &'a Prune,
    ) -> BoxFuture<'a, Result<Emptied, StorageError>> {
        Box::pin(async {
            Err(StorageError::Unsupported(
                "KMS keeps keys and aliases only, nothing that grows: schedule a key's deletion through KMS, or reset KMS".to_owned(),
            ))
        })
    }

    fn reset(&self, dry_run: bool) -> BoxFuture<'_, Result<Emptied, String>> {
        Box::pin(async move {
            let mut total = Emptied::default();
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Reset { dry_run }).await {
                    Some(DataReply::Emptied(Ok(done))) => total.add(done),
                    Some(DataReply::Emptied(Err(e))) => return Err(e),
                    _ => return Err(format!("KMS in {region} did not answer")),
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
                    _ => return Err(format!("KMS in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
