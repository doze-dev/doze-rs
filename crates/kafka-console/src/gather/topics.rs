//! The topics list: names, partitions, policy, size and last write, from Metadata, DescribeConfigs and the files.

use super::configs::{self, TopicConfigs};
use super::{meta, readers, txns};
use crate::backend::Backend;
use crate::model::{TopicRow, TopicsView};
use crate::source::now_ms;

/// The most rows a page shows.
const MOST_ROWS: usize = 200;

/// The topics whose name contains `filter`, as the list shows them.
pub async fn list(b: &Backend, filter: &str, show_internal: bool) -> TopicsView {
    let filter: String = filter.chars().take(249).collect();
    let now = now_ms();
    let all = meta::all(b).await.unwrap_or_default();
    let user: Vec<&meta::TopicMeta> = all.iter().filter(|t| !t.internal).collect();
    let partitions: usize = user.iter().map(|t| t.partitions.len()).sum();
    let matched: Vec<&meta::TopicMeta> = user
        .iter()
        .copied()
        .filter(|t| filter.is_empty() || t.name.contains(filter.as_str()))
        .collect();
    let truncated = matched.len() > MOST_ROWS;
    let shown: Vec<&meta::TopicMeta> = matched.into_iter().take(MOST_ROWS).collect();
    let names: Vec<String> = shown.iter().map(|t| t.name.clone()).collect();
    let configs = configs::of(b, &names).await;
    let ongoing = txns::ongoing(b).await;
    let readers = readers::all(b).await;
    let mut rows = Vec::new();
    for t in shown {
        let c: TopicConfigs = configs.get(&t.name).copied().unwrap_or_default();
        let files = b.partition_files(&t.name).unwrap_or_default();
        let readable = files.iter().all(|f| !f.unreadable);
        let bytes: u64 = files.iter().map(|f| f.bytes).sum();
        rows.push(TopicRow {
            name: t.name.clone(),
            internal: false,
            partitions: i32::try_from(t.partitions.len()).unwrap_or(i32::MAX),
            cleanup: c.cleanup,
            txn_open: ongoing
                .iter()
                .any(|o| o.topics.iter().any(|(topic, _)| *topic == t.name)),
            empty: readable && bytes == 0,
            retention_ms: c.retention_set.then_some(c.retention_ms),
            // Only a topic with more segments than partitions has something to say: each partition has one.
            segments: readable
                .then(|| files.iter().map(|f| f.segments).sum())
                .filter(|n: &usize| *n > t.partitions.len()),
            read_by: readers
                .by_topic
                .get(&t.name)
                .map(|g| g.iter().cloned().collect())
                .unwrap_or_default(),
            bytes: readable.then_some(bytes),
            last_write_ms: files.iter().filter_map(|f| f.last_write_ms).max(),
            offline: files.iter().filter(|f| f.offline.is_some()).count(),
        });
    }
    let newest = filter.is_empty().then(|| {
        rows.iter()
            .filter_map(|r| r.last_write_ms.map(|ms| (r.name.clone(), ms)))
            .max_by_key(|(_, ms)| *ms)
    });
    if show_internal {
        // Metadata never lists Kafka's own topic: the console knows its name and how many partitions it stands for.
        rows.push(TopicRow::internal());
    }
    TopicsView {
        rows,
        total: user.len(),
        partitions,
        filter,
        show_internal,
        groups: readers.groups,
        newest: newest.flatten(),
        now_ms: now,
        truncated,
    }
}
