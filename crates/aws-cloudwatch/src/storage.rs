//! CloudWatch's data for a person to see and remove (`doze-aws data`). A resource is a metric namespace; its bytes are its metrics'
//! stored data points (one row per metric, minute or second, and unit) and the metrics' own rows. Emptying removes the data points,
//! whole or those before a time; a metric left with no data is removed with them, as on AWS a metric with no data stops being
//! listed. Alarms and dashboards are definitions: they stay.
//!
//! Alarm history is a resource of its own, named `(alarm history)` (no namespace can hold a parenthesis): it belongs to alarms,
//! not to a namespace (a composite alarm has none), and emptying it, whole or before a time, keeps the alarms.

use std::collections::BTreeMap;

use aws_wire::service::BoxFuture;
use aws_wire::storage::{Emptied, File, Names, Prune, Resource, Storage, StorageError, Usage};
use doze_store::record;
use tokio::sync::oneshot;

use crate::Cloudwatch;
use crate::core::{Core, Request, SAMPLES, VERSION};
use crate::model::{History, sample_key};

/// The resource alarm history is reported as.
pub(crate) const ALARM_HISTORY: &str = "(alarm history)";

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

/// A sample row's series number and bucket start, read from its key.
fn key_parts(key: &str) -> Option<(u32, i64)> {
    let no = u32::from_str_radix(key.get(..8)?, 16).ok()?;
    let at = i64::from_str_radix(key.get(9..25)?, 16).ok()?;
    Some((no, at))
}

fn encoded<T: serde::Serialize>(what: &str, value: &T) -> u64 {
    record::encode(what, VERSION, value).map_or(0, |b| b.len() as u64)
}

/// What a namespace holds: rows and bytes of data points, by series number.
#[derive(Default)]
struct Points {
    by_series: BTreeMap<u32, (u64, u64)>,
}

impl Core {
    /// Answers a data question. Deletes are written and on disk before the file is compacted.
    pub(crate) fn data(&mut self, region: &str, ask: DataAsk) -> DataReply {
        match ask {
            DataAsk::Usage => DataReply::Usage(self.usage(region), self.file(region)),
            DataAsk::Empty { name, prune } => DataReply::Emptied(self.empty(&name, &prune)),
            DataAsk::Reset { dry_run } => {
                let done = self.reset(dry_run);
                if !dry_run {
                    self.forget_watched(region);
                }
                DataReply::Emptied(Ok(done))
            }
            DataAsk::Compact => DataReply::Compacted(self.compact()),
        }
    }

    /// The data points before `before` (all of them when `None`), by series.
    fn points(&self, before: Option<i64>) -> Points {
        let mut points = Points::default();
        let mut each = |key: &str, len: usize| {
            if let Some((no, at)) = key_parts(key)
                && before.is_none_or(|b| at < b)
            {
                let slot = points.by_series.entry(no).or_default();
                slot.0 += 1;
                slot.1 += (key.len() + len) as u64;
            }
        };
        match &self.db {
            Some(db) => {
                // WHY: a failed read reports what was counted; the next usage reads again.
                drop(db.scan(SAMPLES, "", |k, v| {
                    each(k, v.len());
                    true
                }));
            }
            None => {
                for (k, v) in self
                    .samples
                    .scan(None, "", "\u{10ffff}", usize::MAX, false)
                    .unwrap_or_default()
                {
                    each(&k, v.len());
                }
            }
        }
        points
    }

    fn usage(&self, region: &str) -> Vec<Resource> {
        let points = self.points(None);
        let mut by: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for (key, s) in self.series.iter() {
            let slot = by.entry(s.namespace.clone()).or_default();
            let (rows, bytes) = points.by_series.get(&s.no).copied().unwrap_or_default();
            slot.0 += rows;
            slot.1 += bytes + (key.len() as u64) + encoded("metric", s);
        }
        let mut resources: Vec<Resource> = by
            .into_iter()
            .map(|(name, (items, bytes))| Resource {
                region: region.to_owned(),
                name,
                bytes,
                items,
                notes: Vec::new(),
            })
            .collect();
        let (items, bytes) = self.history_size(None);
        if items > 0 {
            resources.push(Resource {
                region: region.to_owned(),
                name: ALARM_HISTORY.to_owned(),
                bytes,
                items,
                notes: Vec::new(),
            });
        }
        resources
    }

    /// The alarm history entries before `before` (all when `None`), and the bytes of the rows that hold them.
    fn history_size(&self, before: Option<i64>) -> (u64, u64) {
        let mut done = (0, 0);
        for (key, h) in self.history.iter() {
            let gone = h
                .items
                .iter()
                .filter(|i| before.is_none_or(|b| i.at_ms < b))
                .count() as u64;
            if gone == 0 {
                continue;
            }
            let whole = (key.len() as u64) + encoded("alarm history", h);
            done.0 += gone;
            done.1 += whole * gone / (h.items.len() as u64).max(1);
        }
        done
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
                "a namespace (or the alarm history) can be emptied whole, or of the data before a time (--older-than)"
                    .to_owned(),
            ));
        }
        if name == ALARM_HISTORY {
            return self.empty_history(prune);
        }
        let series: Vec<(String, u32, i64)> = self
            .series
            .iter()
            .filter(|(_, s)| s.namespace == name)
            .map(|(k, s)| (k.to_owned(), s.no, s.last_ms))
            .collect();
        if series.is_empty() {
            return Err(StorageError::NotFound);
        }
        let points = self.points(prune.before_ms);
        let mut done = Emptied::default();
        for (key, no, last_ms) in &series {
            let (rows, bytes) = points.by_series.get(no).copied().unwrap_or_default();
            done.items += rows;
            done.bytes += bytes;
            if prune.before_ms.is_none_or(|b| *last_ms < b)
                && let Some(s) = self.series.get(key)
            {
                done.bytes += (key.len() as u64) + encoded("metric", s);
            }
        }
        if prune.dry_run || done.bytes == 0 {
            return Ok(done);
        }
        for (key, no, last_ms) in series {
            match prune.before_ms {
                Some(before) if last_ms >= before => {
                    for high in [false, true] {
                        self.samples.delete_range(
                            &sample_key(no, high, 0, 0),
                            &sample_key(no, high, before, 0),
                        );
                    }
                }
                _ => {
                    self.series.remove(&key);
                    self.samples
                        .delete_range(&format!("{no:08x}"), &format!("{:08x}", no + 1));
                }
            }
        }
        done.freed = self.settle_data().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn empty_history(&mut self, prune: &Prune) -> Result<Emptied, StorageError> {
        let (items, bytes) = self.history_size(prune.before_ms);
        let mut done = Emptied {
            items,
            bytes,
            freed: 0,
        };
        if prune.dry_run || items == 0 {
            return Ok(done);
        }
        let keys: Vec<String> = self.history.iter().map(|(k, _)| k.to_owned()).collect();
        for key in keys {
            let Some(h) = self.history.get(&key) else {
                continue;
            };
            let kept: Vec<_> = h
                .items
                .iter()
                .filter(|i| prune.before_ms.is_some_and(|b| i.at_ms >= b))
                .cloned()
                .collect();
            if kept.len() == h.items.len() {
                continue;
            }
            if kept.is_empty() {
                self.history.remove(&key);
            } else {
                self.history.insert(key, History { items: kept });
            }
        }
        done.freed = self.settle_data().map_err(StorageError::Failed)?;
        Ok(done)
    }

    fn reset(&mut self, dry_run: bool) -> Emptied {
        let points = self.points(None);
        let (h_items, h_bytes) = self.history_size(None);
        let mut done = Emptied {
            items: h_items,
            bytes: h_bytes,
            freed: 0,
        };
        for (rows, bytes) in points.by_series.values() {
            done.items += rows;
            done.bytes += bytes;
        }
        if dry_run {
            return done;
        }
        clear(&mut self.series);
        clear(&mut self.alarms);
        clear(&mut self.history);
        clear(&mut self.docs);
        self.samples.delete_range("", "\u{10ffff}");
        self.due.clear();
        self.outbox.clear();
        self.to_evaluate.clear();
        self.trimmed_ms = 0;
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

/// Removes every record of a table.
fn clear<T: serde::Serialize + serde::de::DeserializeOwned>(
    records: &mut doze_store::records::Records<T>,
) {
    let keys: Vec<String> = records.iter().map(|(k, _)| k.to_owned()).collect();
    for key in keys {
        records.remove(&key);
    }
}

impl Cloudwatch {
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

impl Storage for Cloudwatch {
    fn names(&self) -> Names {
        Names {
            one: "namespace",
            many: "namespaces",
            units: "data points",
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
                    _ => return Err(format!("CloudWatch in {region} did not answer")),
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
                    "CloudWatch in {region} did not answer"
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
                    _ => return Err(format!("CloudWatch in {region} did not answer")),
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
                    _ => return Err(format!("CloudWatch in {region} did not answer")),
                }
            }
            Ok(freed)
        })
    }
}
