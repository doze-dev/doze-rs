//! S3's data for a person to see and remove (`doze-aws data`): each bucket's objects, old versions, delete markers and unfinished
//! uploads, emptied whole or by prefix, by time, of old versions only or of unfinished uploads only; the bucket and its settings
//! stay. The removal itself is in `data.rs`, inside the region's actor.

use aws_wire::service::BoxFuture;
use aws_wire::storage::{Emptied, File, Names, Prune, Resource, Storage, StorageError, Usage};
use tokio::sync::oneshot;

use crate::S3;
use crate::core::Core;
use crate::req::{Answer, Ask, Out};

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
    Reset(Result<Emptied, String>),
    Compacted(Result<u64, String>),
}

/// A data question and where its answer goes.
#[derive(Debug)]
pub(crate) struct Asked {
    pub(crate) region: String,
    pub(crate) ask: DataAsk,
    pub(crate) reply: oneshot::Sender<DataReply>,
}

impl Core {
    /// Answers a data question.
    pub(crate) async fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region).map(|r| (r, self.file(region)))),
            DataAsk::Empty { name, prune } => {
                DataReply::Emptied(self.empty_bucket(&name, &prune).await)
            }
            DataAsk::Reset { dry_run } => DataReply::Reset(self.reset(dry_run).await),
            DataAsk::Compact => DataReply::Compacted(self.compact()),
        }
    }

    /// Answers a data question asked of the actor; the answer travels on the question's own channel.
    pub(crate) async fn data_asked(&mut self, asked: Asked) -> Answer {
        let Asked { region, ask, reply } = asked;
        if reply.send(self.data(&region, ask).await).is_err() {
            // WHY: the person stopped waiting; what was asked was done all the same.
        }
        Out::empty()
    }
}

impl S3 {
    async fn ask_data(&self, region: &str, ask: DataAsk) -> Option<DataReply> {
        let (reply, answer) = oneshot::channel();
        let asked = Asked {
            region: region.to_owned(),
            ask,
            reply,
        };
        self.inner
            .ask(region, Ask::Data(Box::new(asked)))
            .await
            .ok()?;
        answer.await.ok()
    }

    fn regions(&self) -> Vec<String> {
        self.inner.actors.keys_on_disk()
    }
}

fn silent(region: &str) -> String {
    format!("S3 in {region} did not answer")
}

impl Storage for S3 {
    fn names(&self) -> Names {
        Names {
            one: "bucket",
            many: "buckets",
            units: "objects",
            prunes: &["before", "prefix", "old-versions", "unused"],
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
                    Some(DataReply::Usage(Err(e))) => return Err(format!("S3 in {region}: {e}")),
                    _ => return Err(silent(&region)),
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
                _ => Err(StorageError::Failed(silent(region))),
            }
        })
    }

    fn reset(&self, dry_run: bool) -> BoxFuture<'_, Result<Emptied, String>> {
        Box::pin(async move {
            let mut total = Emptied::default();
            for region in self.regions() {
                match self.ask_data(&region, DataAsk::Reset { dry_run }).await {
                    Some(DataReply::Reset(Ok(done))) => total.add(done),
                    Some(DataReply::Reset(Err(e))) => return Err(format!("S3 in {region}: {e}")),
                    _ => return Err(silent(&region)),
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
                    _ => return Err(silent(&region)),
                }
            }
            Ok(freed)
        })
    }
}
