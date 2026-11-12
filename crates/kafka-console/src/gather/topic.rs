//! What every page of one topic says about it: partitions with their offsets and files, its configs, the transactions
//! open on it and the counts on its tabs.

use kafka_wire::api::Response;

use super::configs::{self, TopicConfigs};
use super::meta::{self, TopicMeta};
use super::{readers, txns};
use crate::backend::{Backend, requests};
use crate::model::{OpenTxn, PartitionRow, TabCounts, TopicHead};
use crate::source::now_ms;

/// The offsets of the partitions: `(start, end, last stable)` in partition order, by one ListOffsets for the log start and
/// the log end and one for the last stable offset. A partition the broker could not answer reads 0, 0, 0.
async fn bounds(b: &Backend, topic: &str, parts: &[i32]) -> Vec<(i64, i64, i64)> {
    use requests::{Ask, EARLIEST, Isolation, LATEST};
    let mut asks = Vec::new();
    for &p in parts {
        asks.push(Ask {
            partition: p,
            timestamp: EARLIEST,
        });
        asks.push(Ask {
            partition: p,
            timestamp: LATEST,
        });
    }
    let answer = |r: Option<Response>| -> Vec<i64> {
        let Some(Response::ListOffsets(r)) = r else {
            return Vec::new();
        };
        r.topics
            .first()
            .map(|t| {
                t.partitions
                    .iter()
                    .map(|p| if p.error_code == 0 { p.offset } else { 0 })
                    .collect()
            })
            .unwrap_or_default()
    };
    let both = answer(
        b.call(requests::list_offsets(
            topic,
            &asks,
            Isolation::ReadUncommitted,
        ))
        .await
        .ok(),
    );
    let latest: Vec<requests::Ask> = parts
        .iter()
        .map(|&p| Ask {
            partition: p,
            timestamp: LATEST,
        })
        .collect();
    let stable = answer(
        b.call(requests::list_offsets(
            topic,
            &latest,
            Isolation::ReadCommitted,
        ))
        .await
        .ok(),
    );
    (0..parts.len())
        .map(|i| {
            let start = both.get(2 * i).copied().unwrap_or(0).max(0);
            let end = both.get(2 * i + 1).copied().unwrap_or(0).max(0);
            let lso = stable
                .get(i)
                .copied()
                .unwrap_or(end)
                .clamp(start, end.max(start));
            (start, end, lso)
        })
        .collect()
}

/// The producers active on the topic: how many, and for each `(partition, producer id, open transaction's first offset)`.
async fn producers(b: &Backend, topic: &str, parts: &[i32]) -> (usize, Vec<(i32, i64, i64)>) {
    let Ok(Response::DescribeProducers(r)) =
        b.call(requests::describe_producers(topic, parts)).await
    else {
        return (0, Vec::new());
    };
    let mut ids = std::collections::BTreeSet::new();
    let mut open = Vec::new();
    for p in r
        .topics
        .iter()
        .flat_map(|t| &t.partitions)
        .filter(|p| p.error_code == 0)
    {
        for x in &p.active_producers {
            ids.insert(x.producer_id.0);
            if x.current_txn_start_offset >= 0 {
                open.push((
                    p.partition_index,
                    x.producer_id.0,
                    x.current_txn_start_offset,
                ));
            }
        }
    }
    (ids.len(), open)
}

fn open_txns(topic: &str, all: &[txns::Open], starts: &[(i32, i64, i64)]) -> Vec<OpenTxn> {
    all.iter()
        .filter_map(|t| {
            let parts = &t.topics.iter().find(|(name, _)| name == topic)?.1;
            Some(OpenTxn {
                id: t.id.clone(),
                producer_id: t.producer_id,
                producer_epoch: t.producer_epoch,
                since_ms: t.since_ms,
                timeout_ms: t.timeout_ms,
                partitions: parts
                    .iter()
                    .map(|&p| {
                        let first = starts
                            .iter()
                            .find(|(sp, pid, _)| *sp == p && *pid == t.producer_id)
                            .map(|(_, _, o)| *o);
                        (p, first)
                    })
                    .collect(),
            })
        })
        .collect()
}

/// The head of a topic already looked up in Metadata.
pub(super) async fn head_of(b: &Backend, meta: &TopicMeta) -> TopicHead {
    let name = meta.name.as_str();
    let configs = configs::of(b, std::slice::from_ref(&meta.name)).await;
    let c: TopicConfigs = configs.get(name).copied().unwrap_or_default();
    let offsets = bounds(b, name, &meta.partitions).await;
    let files = b.partition_files(name).unwrap_or_default();
    let (producer_count, starts) = producers(b, name, &meta.partitions).await;
    let ongoing = txns::ongoing(b).await;
    let readers = readers::all(b).await;
    let parts = meta
        .partitions
        .iter()
        .zip(&offsets)
        .map(|(&partition, &(start, end, last_stable))| {
            let f = files.iter().find(|f| f.partition == partition);
            PartitionRow {
                partition,
                start,
                end,
                last_stable,
                bytes: f.filter(|f| !f.unreadable).map(|f| f.bytes),
                last_write_ms: f.and_then(|f| f.last_write_ms),
                offline: f.and_then(|f| f.offline.clone()),
            }
        })
        .collect();
    TopicHead {
        name: meta.name.clone(),
        cleanup: c.cleanup,
        retention_ms: Some(c.retention_ms),
        parts,
        txns: open_txns(name, &ongoing, &starts),
        tabs: TabCounts {
            consumers: readers
                .by_topic
                .get(name)
                .map_or(0, std::collections::BTreeSet::len),
            producers: producer_count,
        },
        now_ms: now_ms(),
    }
}

/// The head of the topic called `name`, if it exists and the console may describe it.
pub async fn head(b: &Backend, name: &str) -> Option<TopicHead> {
    let meta = meta::one(b, name).await?;
    Some(head_of(b, &meta).await)
}
