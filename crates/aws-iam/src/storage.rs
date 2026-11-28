//! IAM's data for a person to see and remove (`doze-aws data`). IAM keeps definitions only (users, groups, roles, policies, access
//! keys, instance profiles, identity providers), a few KB, and no resource holds data that grows: usage reports its file, emptying is
//! refused with a sentence, and a reset removes everything and makes the account afresh (a new session-signing key, so a session
//! token issued before the reset no longer opens anything), as on a fresh data directory. STS keeps nothing of its own.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{Emptied, File, Names, Prune, Storage, StorageError, Usage};
use tokio::sync::oneshot;

use crate::Iam;
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
    Emptied(Emptied),
    Compacted(Result<u64, String>),
}

/// A data question and where its answer goes.
#[derive(Debug)]
pub(crate) struct DataCall {
    pub(crate) ask: DataAsk,
    pub(crate) reply: oneshot::Sender<DataReply>,
}

/// The region IAM's one file is reported in: IAM is global.
const GLOBAL: &str = "global";

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) async fn data(&mut self, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => {
                let space = self
                    .db
                    .as_ref()
                    .and_then(|db| db.space().ok())
                    .unwrap_or_default();
                DataReply::Usage(File {
                    region: GLOBAL.to_owned(),
                    bytes: space.file,
                    reclaimable: space.reclaimable(),
                })
            }
            DataAsk::Reset { dry_run } => DataReply::Emptied(self.reset(dry_run).await),
            DataAsk::Compact => DataReply::Compacted(self.compact().await),
        }
    }

    async fn reset(&mut self, dry_run: bool) -> Emptied {
        let (items, bytes) = self.count();
        let mut done = Emptied {
            items,
            bytes,
            freed: 0,
        };
        if dry_run {
            return done;
        }
        self.clear_all();
        // WHY: a failed write is the writer's to report on the next call; the counts say what was asked.
        drop(self.flush());
        let db = self.db.take();
        let mode = self.mode;
        *self = Self::fresh(std::sync::Arc::clone(&self.clock), db);
        self.mode = mode;
        drop(self.flush());
        done.freed = self.compact().await.unwrap_or(0);
        done
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

impl Iam {
    async fn ask_data(&self, ask: DataAsk) -> Option<DataReply> {
        let (reply, answer) = oneshot::channel();
        self.0
            .actor
            .call(Ask::Data(Box::new(DataCall { ask, reply })))
            .await
            .ok()?;
        answer.await.ok()
    }

    /// Whether IAM has a file at all: a data question does not make one.
    fn on_disk(&self) -> bool {
        self.0.dir.join(crate::core::FILE).exists()
    }
}

impl Storage for Iam {
    fn names(&self) -> Names {
        Names {
            one: "definition",
            many: "definitions",
            units: "definitions",
            prunes: &[],
        }
    }

    fn usage(&self) -> BoxFuture<'_, Result<Usage, String>> {
        Box::pin(async move {
            let mut usage = Usage::default();
            if !self.on_disk() {
                return Ok(usage);
            }
            match self.ask_data(DataAsk::Usage).await {
                Some(DataReply::Usage(file)) => usage.files.push(file),
                _ => return Err("IAM did not answer".to_owned()),
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
                "IAM keeps definitions only (users, roles, policies, keys), nothing that grows: delete one through IAM, or reset IAM"
                    .to_owned(),
            ))
        })
    }

    fn reset(&self, dry_run: bool) -> BoxFuture<'_, Result<Emptied, String>> {
        Box::pin(async move {
            if !self.on_disk() {
                return Ok(Emptied::default());
            }
            let done = match self.ask_data(DataAsk::Reset { dry_run }).await {
                Some(DataReply::Emptied(done)) => done,
                _ => return Err("IAM did not answer".to_owned()),
            };
            if !dry_run {
                self.0.forget_log();
            }
            Ok(done)
        })
    }

    fn compact(&self) -> BoxFuture<'_, Result<u64, String>> {
        Box::pin(async move {
            if !self.on_disk() {
                return Ok(0);
            }
            match self.ask_data(DataAsk::Compact).await {
                Some(DataReply::Compacted(done)) => done,
                _ => Err("IAM did not answer".to_owned()),
            }
        })
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
