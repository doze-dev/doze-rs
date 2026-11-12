//! The reads the topic forms make: the topic as Metadata says it, its configs, its offsets, who uses it. Each answer
//! is a protocol response; a refusal or a failure to answer is "not known" and the form says less, never more.

use std::collections::BTreeMap;

use kafka_wire::Uuid;
use kafka_wire::api::Response;

use crate::backend::{Backend, requests::forms};
use crate::model::InUse;

/// A topic as Metadata says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopicFacts {
    pub(crate) id: Uuid,
    pub(crate) partitions: Vec<i32>,
}

/// The topic `name`, if the broker has it and the console may describe it.
pub(crate) async fn topic(b: &Backend, name: &str) -> Option<TopicFacts> {
    let Ok(Response::Metadata(m)) = b.call(forms::topic_metadata(name)).await else {
        return None;
    };
    let t = m.topics.into_iter().next()?;
    if t.error_code != 0 {
        return None;
    }
    let mut partitions: Vec<i32> = t.partitions.iter().map(|p| p.partition_index).collect();
    partitions.sort_unstable();
    Some(TopicFacts {
        id: t.topic_id,
        partitions,
    })
}

/// The names of the topics that are there (internal ones included: a name is taken whoever owns it).
pub(crate) async fn topic_names(b: &Backend) -> Vec<String> {
    let Ok(Response::Metadata(m)) = b.call(crate::backend::requests::metadata_all()).await else {
        return Vec::new();
    };
    m.topics
        .into_iter()
        .filter_map(|t| t.name.map(|n| n.0.to_string()))
        .collect()
}

/// The values of these configs on a topic, by key.
pub(crate) async fn configs(b: &Backend, topic: &str, keys: &[&str]) -> BTreeMap<String, String> {
    let Ok(Response::DescribeConfigs(r)) = b.call(forms::topic_configs(topic, keys)).await else {
        return BTreeMap::new();
    };
    r.results
        .into_iter()
        .filter(|res| res.error_code == 0)
        .flat_map(|res| res.configs)
        .filter_map(|c| Some((c.name.to_string(), c.value?.to_string())))
        .collect()
}

/// An offset of each of these partitions (see `forms::offsets_at` for the timestamps), by partition.
pub(crate) async fn offsets(
    b: &Backend,
    topic: &str,
    partitions: &[i32],
    timestamp: i64,
    isolation: i8,
) -> BTreeMap<i32, i64> {
    let Ok(Response::ListOffsets(r)) = b
        .call(forms::offsets_at(topic, partitions, timestamp, isolation))
        .await
    else {
        return BTreeMap::new();
    };
    r.topics
        .into_iter()
        .flat_map(|t| t.partitions)
        .filter(|p| p.error_code == 0)
        .map(|p| (p.partition_index, p.offset))
        .collect()
}

/// How many records a topic holds: the sum over its partitions of the high watermark less the log start.
pub(crate) async fn records(b: &Backend, topic: &str, partitions: &[i32]) -> i64 {
    let start = offsets(b, topic, partitions, -2, 0).await;
    let end = offsets(b, topic, partitions, -1, 0).await;
    end.iter()
        .map(|(p, e)| (e - start.get(p).copied().unwrap_or(*e)).max(0))
        .sum()
}

/// The open transactions that have written to this topic: `(transactional id, partitions of this topic)`.
pub(crate) async fn transactions(b: &Backend, topic: &str) -> Vec<(String, Vec<i32>)> {
    let Ok(Response::ListTransactions(list)) = b
        .call(crate::backend::requests::list_ongoing_transactions())
        .await
    else {
        return Vec::new();
    };
    let ids: Vec<String> = list
        .transaction_states
        .iter()
        .map(|t| t.transactional_id.0.to_string())
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let Ok(Response::DescribeTransactions(d)) = b
        .call(crate::backend::requests::describe_transactions(&ids))
        .await
    else {
        return Vec::new();
    };
    d.transaction_states
        .into_iter()
        .filter(|t| t.error_code == 0)
        .filter_map(|t| {
            let mine: Vec<i32> = t
                .topics
                .iter()
                .filter(|x| x.topic.0.as_str() == topic)
                .flat_map(|x| x.partitions.clone())
                .collect();
            (!mine.is_empty()).then(|| (t.transactional_id.0.to_string(), mine))
        })
        .collect()
}

/// The offsets groups have committed on these partitions of the topic: `(group, partition, offset)`, groups with none left out.
pub(crate) async fn committed_on(
    b: &Backend,
    topic: &str,
    partitions: &[i32],
) -> Vec<(String, i32, i64)> {
    let Ok(Response::ListGroups(list)) = b.call(forms::list_groups()).await else {
        return Vec::new();
    };
    let ids: Vec<String> = list
        .groups
        .iter()
        .map(|g| g.group_id.0.to_string())
        .collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let Ok(Response::OffsetFetch(fetched)) = b
        .call(forms::committed_offsets(&ids, topic, partitions))
        .await
    else {
        return Vec::new();
    };
    fetched
        .groups
        .into_iter()
        .filter(|g| g.error_code == 0)
        .flat_map(|g| {
            let id = g.group_id.0.to_string();
            g.topics
                .into_iter()
                .flat_map(|t| t.partitions)
                .filter(|p| p.error_code == 0 && p.committed_offset >= 0)
                .map(move |p| (id.clone(), p.partition_index, p.committed_offset))
        })
        .collect()
}

/// When the topic was last written: the newest record timestamp over its partitions, if any record has one.
pub(crate) async fn last_write(b: &Backend, topic: &str, partitions: &[i32]) -> Option<i64> {
    let Ok(Response::ListOffsets(r)) = b.call(forms::offsets_at(topic, partitions, -3, 0)).await
    else {
        return None;
    };
    r.topics
        .into_iter()
        .flat_map(|t| t.partitions)
        .filter(|p| p.error_code == 0 && p.timestamp >= 0)
        .map(|p| p.timestamp)
        .max()
}

/// Who is using the topic, as far as the protocol says: the groups with an offset committed on it (and their state and
/// member count), and the open transactions that wrote to it.
pub(crate) async fn in_use(b: &Backend, topic: &str, partitions: &[i32]) -> InUse {
    let mut out = InUse {
        groups: Vec::new(),
        transactions: transactions(b, topic)
            .await
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
    };
    let mut reading: Vec<String> = committed_on(b, topic, partitions)
        .await
        .into_iter()
        .map(|(g, ..)| g)
        .collect();
    reading.sort_unstable();
    reading.dedup();
    if reading.is_empty() {
        return out;
    }
    let Ok(Response::DescribeGroups(d)) = b.call(forms::describe_groups(&reading)).await else {
        return out;
    };
    out.groups = d
        .groups
        .into_iter()
        .filter(|g| g.error_code == 0)
        .map(|g| {
            (
                g.group_id.0.to_string(),
                g.group_state.to_string(),
                g.members.len(),
            )
        })
        .collect();
    out
}
