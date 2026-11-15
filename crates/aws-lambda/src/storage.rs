//! Lambda's data for a person to see and remove (`doze-aws data`): each function's and layer's code and rows, the code nothing uses,
//! and the runtime clients; `unused` removes the last two, and a reset removes everything and ends every function's processes. The
//! counting and removing are in `data.rs`, inside the region's actor.

use std::sync::atomic::Ordering;
use std::time::Duration;

use aws_wire::service::BoxFuture;
use aws_wire::storage::{Emptied, File, Names, Prune, Resource, Storage, StorageError, Usage};
use tokio::sync::oneshot;

use crate::Lambda;
use crate::core::{Answer, Ask, Core};

/// How long code just unpacked is left alone by `unused`: an operation unpacks code before the actor is asked to keep it.
pub(crate) const GRACE_MS: u64 = 60_000;

/// What the actor is asked about its data.
#[derive(Debug)]
pub(crate) enum DataAsk {
    Usage,
    Empty {
        name: String,
        prune: Prune,
        grace: Duration,
    },
    Reset {
        dry_run: bool,
    },
    Compact,
}

/// Its answer.
#[derive(Debug)]
pub(crate) enum DataReply {
    Usage(Vec<Resource>, File),
    Emptied(Result<Emptied, StorageError>),
    /// What a reset did, and the functions whose processes are to be ended.
    Reset(Result<(Emptied, Vec<String>), String>),
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
    /// Answers a data question asked of the actor; the answer travels on the question's own channel.
    pub(crate) async fn data_asked(&mut self, asked: Asked) -> Answer {
        let Asked { region, ask, reply } = asked;
        let answer = match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(&region), self.file(&region)),
            DataAsk::Empty { name, prune, grace } => {
                DataReply::Emptied(self.empty(&name, &prune, grace))
            }
            DataAsk::Reset { dry_run } => DataReply::Reset(self.reset(dry_run).await),
            DataAsk::Compact => DataReply::Compacted(self.compact()),
        };
        if reply.send(answer).is_err() {
            // WHY: the person stopped waiting; what was asked was done all the same.
        }
        Answer::Value(None)
    }
}

impl Lambda {
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

    /// Lets `unused` take code however new it is (a test's code was unpacked a moment ago).
    #[cfg(test)]
    pub(crate) fn no_code_grace(&self) {
        self.inner.code_grace_ms.store(0, Ordering::Relaxed);
    }
}

fn silent(region: &str) -> String {
    format!("Lambda in {region} did not answer")
}

impl Storage for Lambda {
    fn names(&self) -> Names {
        Names {
            one: "function",
            many: "functions",
            units: "code packages",
            prunes: &["unused"],
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
            let grace = Duration::from_millis(self.inner.code_grace_ms.load(Ordering::Relaxed));
            let ask = DataAsk::Empty {
                name: name.to_owned(),
                prune: prune.clone(),
                grace,
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
                    Some(DataReply::Reset(Ok((done, functions)))) => {
                        total.add(done);
                        if !dry_run {
                            for name in functions {
                                self.inner.rt.retire(&format!("{region}/{name}"));
                            }
                            crate::poll::sync(&self.inner, &region).await;
                        }
                    }
                    Some(DataReply::Reset(Err(e))) => {
                        return Err(format!("Lambda in {region}: {e}"));
                    }
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
