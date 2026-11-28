//! The Kinesis poller: it lists the shards of the stream, reads each from the position it holds (a parent to its end before its
//! children, which start at their trim horizon: [`Lineage`]), and hands the records to
//! [`deliver`](super::streams::deliver), which owns the retry, bisect and failure-destination semantics both stream sources share.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Value, json};

use super::Run;
use super::parallel::deliver;
use super::streams::{Delivered, Rec};

/// A Kinesis record as Lambda delivers it.
fn kinesis_record(r: &Value, shard: &str, arn: &str, region: &str, role: &str) -> Rec {
    let text = |k: &str| {
        r.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let seq = text("SequenceNumber");
    let arrival = r
        .get("ApproximateArrivalTimestamp")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let data = text("Data");
    let doc = json!({
        "kinesis": {
            "kinesisSchemaVersion": "1.0",
            "partitionKey": text("PartitionKey"),
            "sequenceNumber": seq,
            "data": data,
            "approximateArrivalTimestamp": arrival,
        },
        "eventSource": "aws:kinesis",
        "eventVersion": "1.0",
        "eventID": format!("{shard}:{seq}"),
        "eventName": "aws:kinesis:record",
        "invokeIdentityArn": role,
        "awsRegion": region,
        "eventSourceARN": arn,
    });
    // Filters see the data as JSON when it is, and the record's fields at the top as well.
    let decoded = aws_core::base64::decode(&data)
        .and_then(|b| String::from_utf8(b).ok())
        .map_or(Value::Null, |s| {
            serde_json::from_str(&s).unwrap_or(Value::String(s))
        });
    let mut filter = doc.clone();
    if let (Some(f), Some(k)) = (
        filter.as_object_mut(),
        doc.get("kinesis").and_then(Value::as_object),
    ) {
        for (key, v) in k {
            f.entry(key.clone()).or_insert_with(|| v.clone());
        }
        f.insert("data".into(), decoded.clone());
        if let Some(Value::Object(inner)) = f.get_mut("kinesis") {
            inner.insert("data".into(), decoded);
        }
    }
    Rec {
        key: text("PartitionKey"),
        seq: aws_kinesis::sequence_value(&seq).unwrap_or(0),
        pos: seq,
        doc,
        filter,
        arrived_ms: (arrival * 1000.0) as i64,
    }
}

/// A shard as `ListShards` names it: its id and the shards it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Shard {
    pub(super) id: String,
    pub(super) parents: Vec<String>,
}

/// The poller of one Kinesis mapping.
pub(super) async fn kinesis(run: Run, kinesis: Arc<aws_kinesis::Kinesis>) {
    let Some(peers) = run.inner.peers.clone() else {
        return;
    };
    let mut poll = Poll {
        arn: run.source_arn().to_owned(),
        role: run.text("FunctionArn").to_owned(),
        batch: super::parallel::read_size(&run, run.int("BatchSize", 100)),
        iterators: run.seed.clone(),
        lineage: Lineage::default(),
        peers,
        run,
    };
    // Woken only by records appended to this mapping's stream; the watch also counts the appends of each shard.
    let watch = kinesis.watch_stream(&poll.arn);
    let (mut shards, mut relist, mut wants) = (Vec::new(), true, Wants::default());
    loop {
        if poll.run.stop.is_cancelled() {
            return;
        }
        // In place before the reads, so records appended between a read and the wait are not missed.
        let appended = watch.notified();
        tokio::pin!(appended);
        appended.as_mut().enable();
        let counts = watch.counts();
        // The shards are listed again only when one the list does not hold took records (a reshard made it) or one was read to its
        // end (its children are ready).
        if relist
            || counts
                .keys()
                .any(|id| !shards.iter().any(|s: &Shard| &s.id == id))
        {
            shards = poll.shards().await;
            relist = false;
        }
        let mut any = false;
        for shard in poll.lineage.ready(&shards) {
            let count = counts.get(&shard.id).copied().unwrap_or(0);
            if !wants.read(&shard.id, count) {
                continue;
            }
            let Some(records) = poll.read(&shard).await else {
                continue;
            };
            relist |= poll.lineage.finished.contains(&shard.id);
            wants.done(&shard.id, count, !records.is_empty());
            if records.is_empty() {
                continue;
            }
            any = true;
            if matches!(
                deliver(&poll.run, "KinesisBatchInfo", &shard.id, records).await,
                Delivered::Stopped
            ) {
                return;
            }
        }
        if !any && !relist {
            tokio::select! {
                () = appended => {}
                () = poll.run.stop.cancelled() => return,
            }
        }
    }
}

/// Which shards a wake reads: one never read, one whose last read found records (more may follow), and one that took records since
/// it was last read (its count of appends moved). A quiet shard of a busy stream is not read on every wake.
#[derive(Debug, Default)]
pub(super) struct Wants {
    /// The count of appends each shard had when it was last read.
    seen: HashMap<String, u64>,
    /// The shards whose last read found records.
    more: HashSet<String>,
}

impl Wants {
    pub(super) fn read(&self, shard: &str, count: u64) -> bool {
        self.more.contains(shard) || self.seen.get(shard) != Some(&count)
    }

    /// A read of `shard`, begun when its count was `count`, found records or not.
    pub(super) fn done(&mut self, shard: &str, count: u64, found: bool) {
        self.seen.insert(shard.to_owned(), count);
        if found {
            self.more.insert(shard.to_owned());
        } else {
            self.more.remove(shard);
        }
    }
}

/// What the poller knows of the shards' history, which is what keeps the records of one partition key in order across a reshard: a
/// child is not read until every parent it has is read to its end, and a child whose parent this mapping was following starts at
/// `TRIM_HORIZON`, so none of its records is skipped (AWS Lambda Developer Guide, "Polling and stream starting positions": Lambda
/// processes parent shards before child shards). A parent that was already closed when the mapping first read it was not being followed
/// (with `LATEST` nothing of it was due), and its children start at the mapping's own starting position.
#[derive(Debug, Default)]
pub(super) struct Lineage {
    /// Shards read to their end.
    finished: HashSet<String>,
    /// Shards this mapping held a place in while they were open (or holds a saved position in).
    followed: HashSet<String>,
}

impl Lineage {
    /// The listed shards that may be read now: not finished, and with every listed parent finished.
    pub(super) fn ready(&self, shards: &[Shard]) -> Vec<Shard> {
        let listed: HashSet<&str> = shards.iter().map(|s| s.id.as_str()).collect();
        shards
            .iter()
            .filter(|s| !self.finished.contains(&s.id))
            .filter(|s| {
                s.parents
                    .iter()
                    .all(|p| !listed.contains(p.as_str()) || self.finished.contains(p))
            })
            .cloned()
            .collect()
    }

    /// Whether a shard with no place of its own starts at the trim horizon: one of its parents was followed.
    pub(super) fn starts_at_horizon(&self, shard: &Shard) -> bool {
        shard.parents.iter().any(|p| self.followed.contains(p))
    }

    /// What one read of `shard` found: whether the shard goes on (`NextShardIterator`), and whether the mapping held a saved position in
    /// it.
    pub(super) fn read(&mut self, shard: &str, goes_on: bool, resumed: bool) {
        if goes_on || resumed {
            self.followed.insert(shard.to_owned());
        }
        if !goes_on {
            self.finished.insert(shard.to_owned());
        }
    }
}

/// What the Kinesis poller holds: the stream, and where it is in each shard.
struct Poll {
    run: Run,
    peers: Arc<aws_peers::Peers>,
    arn: String,
    role: String,
    batch: i64,
    iterators: HashMap<String, String>,
    lineage: Lineage,
}

impl Poll {
    async fn kinesis(&self, op: &'static str, body: Value) -> Option<Value> {
        self.peers
            .call("lambda", &self.run.caller, "kinesis", op, &body)
            .await
            .ok()
    }

    /// The shards of the stream, with their parents.
    async fn shards(&self) -> Vec<Shard> {
        let listed = self
            .kinesis("ListShards", json!({"StreamARN": self.arn}))
            .await;
        listed
            .and_then(|l| l.get("Shards").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|s| {
                let text = |k: &str| s.get(k).and_then(Value::as_str).map(str::to_owned);
                Some(Shard {
                    id: text("ShardId")?,
                    parents: [text("ParentShardId"), text("AdjacentParentShardId")]
                        .into_iter()
                        .flatten()
                        .collect(),
                })
            })
            .collect()
    }

    /// The next records of a shard, and the iterator after them kept; `None` when the shard could not be read (it is read again on the
    /// next wake, as before).
    async fn read(&mut self, shard: &Shard) -> Option<Vec<Rec>> {
        let horizon = self.lineage.starts_at_horizon(shard);
        let id = shard.id.as_str();
        let iterator = shard_iterator(
            &self.run,
            &self.peers,
            &self.arn,
            id,
            &mut self.iterators,
            horizon,
        )
        .await?;
        let ask = json!({"StreamARN": self.arn, "ShardIterator": iterator, "Limit": self.batch});
        let Some(got) = self.kinesis("GetRecords", ask).await else {
            self.iterators.remove(id);
            return None;
        };
        let next = got.get("NextShardIterator").and_then(Value::as_str);
        // A shard that ends at its first read was still followed if the mapping holds a saved position in it (from before a restart).
        let resumed = next.is_none()
            && !self.lineage.followed.contains(id)
            && self.run.position(id).await.is_some();
        self.lineage.read(id, next.is_some(), resumed);
        match next {
            Some(next) => {
                self.iterators.insert(id.to_owned(), next.to_owned());
            }
            None => {
                self.iterators.remove(id);
            }
        }
        let records = got
            .get("Records")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        Some(
            records
                .map(|r| kinesis_record(r, id, &self.arn, &self.run.region, &self.role))
                .collect(),
        )
    }
}

/// The iterator for a shard: the one held, or one made from the saved position, or from the trim horizon for a child whose parent
/// was followed, or from the mapping's starting position.
pub(super) async fn shard_iterator(
    run: &Run,
    peers: &aws_peers::Peers,
    arn: &str,
    shard: &str,
    held: &mut HashMap<String, String>,
    horizon: bool,
) -> Option<String> {
    if let Some(it) = held.get(shard) {
        return Some(it.clone());
    }
    let mut ask = json!({"StreamARN": arn, "ShardId": shard});
    match run.position(shard).await {
        Some(pos) => {
            crate::set(
                &mut ask,
                "ShardIteratorType",
                json!("AFTER_SEQUENCE_NUMBER"),
            );
            crate::set(&mut ask, "StartingSequenceNumber", json!(pos));
        }
        None if horizon => crate::set(&mut ask, "ShardIteratorType", json!("TRIM_HORIZON")),
        None => {
            let start = run
                .cfg
                .get("StartingPosition")
                .and_then(Value::as_str)
                .unwrap_or("LATEST");
            crate::set(&mut ask, "ShardIteratorType", json!(start));
            if start == "AT_TIMESTAMP" {
                crate::set(
                    &mut ask,
                    "Timestamp",
                    run.cfg
                        .get("StartingPositionTimestamp")
                        .cloned()
                        .unwrap_or(json!(0)),
                );
            }
        }
    }
    let got = peers
        .call("lambda", &run.caller, "kinesis", "GetShardIterator", &ask)
        .await
        .ok()?;
    let it = got.get("ShardIterator")?.as_str()?.to_owned();
    held.insert(shard.to_owned(), it.clone());
    Some(it)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shard(id: &str, parents: &[&str]) -> Shard {
        Shard {
            id: id.to_owned(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    fn ids(shards: &[Shard]) -> Vec<&str> {
        shards.iter().map(|s| s.id.as_str()).collect()
    }

    /// Before: every wake listed the shards and read every ready one; a quiet shard of a busy stream was read each time.
    #[test]
    fn a_wake_reads_the_shards_that_took_records_and_those_with_more() {
        let mut w = Wants::default();
        assert!(
            w.read("a", 0) && w.read("b", 0),
            "each is read once at first"
        );
        w.done("a", 0, false);
        w.done("b", 0, true);
        assert!(!w.read("a", 0), "quiet: not read again");
        assert!(
            w.read("b", 0),
            "its last read found records: more may follow"
        );
        assert!(w.read("a", 3), "records were appended to it since");
        w.done("b", 0, false);
        assert!(!w.read("b", 0));
    }

    #[test]
    fn children_wait_for_their_parents_and_start_at_the_horizon_when_a_parent_was_followed() {
        let listed = [shard("p", &[]), shard("a", &["p"]), shard("b", &["p"])];
        let mut l = Lineage::default();
        assert_eq!(ids(&l.ready(&listed)), ["p"], "children wait");
        l.read("p", true, false);
        assert_eq!(ids(&l.ready(&listed)), ["p"], "the parent is not done yet");
        l.read("p", false, false);
        assert_eq!(ids(&l.ready(&listed)), ["a", "b"]);
        assert!(l.starts_at_horizon(&listed[1]));
        // A merge waits for both parents.
        let merged = [shard("a", &[]), shard("b", &[]), shard("m", &["a", "b"])];
        let mut l = Lineage::default();
        l.read("a", false, false);
        assert_eq!(ids(&l.ready(&merged)), ["b"]);
        l.read("b", false, false);
        assert_eq!(ids(&l.ready(&merged)), ["m"]);
        assert!(
            !l.starts_at_horizon(&merged[2]),
            "neither parent was followed: the mapping's own start"
        );
        // A parent trimmed out of the list does not hold its child.
        assert_eq!(
            ids(&Lineage::default().ready(&[shard("c", &["gone"])])),
            ["c"]
        );
    }
}
