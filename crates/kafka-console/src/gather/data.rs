//! The Data page from the broker: its usage of the data directory, the transactions under way, the groups that read each
//! topic, and the growth the console's own looks measure (docs/port/data-control.md).

use kafka_broker::{DataRefusal, DataUsage, TopicUsage};

use super::{configs, forms, readers, txns};
use crate::backend::Backend;
use crate::data_words;
use crate::growth::{Growth, Rates};
use crate::model::{DataTopicRow, DataView, EmptyTopicView, VolumeView};
use crate::source::now_ms;

/// The Data page; `None` if the console may not describe the cluster. `subjects` is the registry's count, if there is one.
pub async fn view(b: &Backend, growth: &Growth, subjects: Option<usize>) -> Option<DataView> {
    let usage = b.data_usage().await?;
    let ongoing = txns::ongoing(b).await;
    let readers = readers::all(b).await;
    let now = now_ms();
    let sizes: Vec<(String, u64)> = usage
        .topics
        .iter()
        .map(|t| (t.name.clone(), t.bytes))
        .collect();
    let rates = growth.note(now, &sizes);
    let alarm = data_words::alarm(&usage);
    growth.set_alarm(alarm.clone());
    let total = usage.total();
    let topics = usage
        .topics
        .iter()
        .map(|t| {
            let mut row = row(t, total, now, &rates);
            row.txn_open = ongoing
                .iter()
                .any(|o| o.topics.iter().any(|(name, _)| *name == t.name));
            row.read_by = readers
                .by_topic
                .get(&t.name)
                .map(|g| g.iter().cloned().collect())
                .unwrap_or_default();
            row
        })
        .collect();
    Some(
        assemble(&usage, topics, subjects, now, (rates, alarm))
            .with_txn(ongoing.first().map(|o| o.id.clone())),
    )
}

/// The heads-up for Home: one look at the data directory (counted toward growth as the page's are), nothing else asked.
pub async fn alarm(b: &Backend, growth: &Growth) -> Option<crate::model::DiskAlarm> {
    let usage = b.data_usage().await?;
    let sizes: Vec<(String, u64)> = usage
        .topics
        .iter()
        .map(|t| (t.name.clone(), t.bytes))
        .collect();
    growth.note(now_ms(), &sizes);
    let alarm = data_words::alarm(&usage);
    growth.set_alarm(alarm.clone());
    alarm
}

/// The page from a usage and its rows: what the live broker and the fixtures share.
#[must_use]
pub fn assemble(
    usage: &DataUsage,
    topics: Vec<DataTopicRow>,
    subjects: Option<usize>,
    now: i64,
    (rates, alarm): (Rates, Option<crate::model::DiskAlarm>),
) -> DataView {
    DataView {
        now_ms: now,
        data_dir: usage.data_dir.display().to_string(),
        total: usage.total(),
        cut: usage.cut,
        volume: usage.space.map(|s| VolumeView {
            total: s.total,
            free: s.free,
        }),
        kinds: data_words::kinds(usage, subjects),
        topics,
        hidden_bytes: usage.hidden_bytes,
        strays: usage
            .strays
            .iter()
            .map(|s| (s.name.clone(), s.bytes))
            .collect(),
        defaults: data_words::defaults_line(&usage.defaults),
        alarm,
        groups: usage.groups,
        transactional_ids: usage.transactional_ids,
        subjects,
        txn_under_way: None,
        growth_since_ms: rates.since_ms,
    }
}

impl DataView {
    /// The same page, saying the transaction under way, if there is one.
    #[must_use]
    pub fn with_txn(mut self, id: Option<String>) -> Self {
        self.txn_under_way = id;
        self
    }
}

/// One topic's row: its share of `total`, what it keeps and why, its growth.
#[must_use]
pub fn row(t: &TopicUsage, total: u64, now: i64, rates: &Rates) -> DataTopicRow {
    DataTopicRow {
        name: t.name.clone(),
        partitions: t.partitions,
        bytes: t.bytes,
        share_permille: data_words::share(t.bytes, total),
        segments: t.segments,
        oldest_ms: t.oldest_ms,
        last_write_ms: t.last_write_ms,
        cleanup: data_words::cleanup(&t.keeps),
        retention: data_words::retention_short(&t.keeps),
        own_retention: t.keeps.own,
        unbounded: t.keeps.unbounded(),
        why: t.keeps.why(t.next_drop_ms.map(|d| d - now)),
        next_drop_ms: t.next_drop_ms,
        growth_per_hour: rates.per_hour.get(&t.name).copied(),
        txn_open: false,
        read_by: Vec::new(),
    }
}

/// What Empty topic will remove; `None` if there is no such topic (or the console may not describe it).
pub async fn empty_form(b: &Backend, topic: &str) -> Option<EmptyTopicView> {
    let facts = forms::topic(b, topic).await?;
    let policy = forms::configs(b, topic, &["cleanup.policy"]).await;
    let cleanup = configs::cleanup_of(
        policy
            .get("cleanup.policy")
            .map_or("delete", String::as_str),
    );
    let in_use = forms::in_use(b, topic, &facts.partitions).await;
    let blocked = in_use.transactions.first().map(|id| {
        DataRefusal::TxnOpen {
            topic: Some(topic.to_owned()),
            id: Some(id.clone()),
        }
        .sentence()
    });
    let bytes = b
        .partition_files(topic)
        .map_or(0, |files| files.iter().map(|f| f.bytes).sum());
    Some(EmptyTopicView {
        topic: topic.to_owned(),
        partitions: facts.partitions.len(),
        bytes,
        records: forms::records(b, topic, &facts.partitions).await,
        cleanup,
        in_use: data_words::empty_in_use(&in_use),
        keeps: data_words::empty_keeps(cleanup != crate::model::Cleanup::Delete),
        blocked,
    })
}
