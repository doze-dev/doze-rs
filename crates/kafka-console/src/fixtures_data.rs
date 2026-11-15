//! The larder's Data page: the topics of `fixtures_topics` on a 250 GiB disk, through the same words a live broker's go
//! through (`gather::data::assemble`), and canned answers for the three actions.

use kafka_broker::{DataRefusal, DataUsage, Emptied, EmptiedAll, Erased, Keeps, Stray, TopicUsage};
use kafka_storage::fs::Space;

use crate::gather::data::{assemble, row};
use crate::growth::Rates;
use crate::model::{Cleanup, DataView, EmptyTopicView};
use crate::source::now_ms;
use crate::{data_words, fixtures_topics};

const MIB: u64 = 1 << 20;
const DAY: i64 = 86_400_000;

fn keeps(cleanup: Cleanup, age: Option<i64>) -> Keeps {
    Keeps {
        compact: cleanup != Cleanup::Delete,
        delete: cleanup != Cleanup::Compact,
        max_age_ms: age.filter(|_| cleanup != Cleanup::Compact),
        max_bytes: None,
        own: age.is_some(),
    }
}

/// The larder's usage: every topic of the list, a metadata store, a directory left by a lost store.
#[must_use]
pub fn usage() -> DataUsage {
    let now = now_ms();
    let list = fixtures_topics::topics("", false, None);
    let mut topics: Vec<TopicUsage> = list
        .rows
        .iter()
        .map(|r| {
            let age = r
                .retention_ms
                .or((r.name == "payments.settled").then_some(7 * DAY));
            TopicUsage {
                name: r.name.clone(),
                partitions: r.partitions,
                bytes: r.bytes.unwrap_or(0),
                segments: r.segments.unwrap_or(1),
                oldest_ms: (r.bytes != Some(0)).then(|| now - 9 * DAY),
                last_write_ms: r.last_write_ms,
                keeps: keeps(r.cleanup, age),
                next_drop_ms: age.map(|_| now + 2 * DAY + 3 * 3_600_000),
            }
        })
        .collect();
    topics.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    DataUsage {
        data_dir: "/Users/dev/.doze/kafka/data".into(),
        space: Some(Space {
            total: 250 << 30,
            free: 61 << 30,
        }),
        topics,
        hidden_bytes: 0,
        strays: vec![Stray {
            name: "audit.trail-0".to_owned(),
            bytes: 18 * MIB,
        }],
        meta_bytes: 3 * MIB / 2,
        other_bytes: 24,
        other_files: 2,
        cut: false,
        groups: 5,
        transactional_ids: 2,
        defaults: keeps(Cleanup::Delete, None),
    }
}

/// The larder's Data page.
#[must_use]
pub fn view() -> DataView {
    let u = usage();
    let now = now_ms();
    let list = fixtures_topics::topics("", false, None);
    let rates = Rates {
        per_hour: [
            ("clickstream", 21 * MIB),
            ("orders", 2 * MIB),
            ("inventory.levels", MIB / 4),
        ]
        .into_iter()
        .map(|(n, b)| (n.to_owned(), i64::try_from(b).unwrap_or(0)))
        .collect(),
        since_ms: Some(now - 14 * 60_000),
    };
    let total = u.total();
    let topics = u
        .topics
        .iter()
        .map(|t| {
            let mut r = row(t, total, now, &rates);
            if let Some(listed) = list.rows.iter().find(|l| l.name == t.name) {
                r.txn_open = listed.txn_open;
                r.read_by.clone_from(&listed.read_by);
            }
            r
        })
        .collect();
    let alarm = data_words::alarm(&u);
    assemble(&u, topics, Some(4), now, (rates, alarm))
}

/// What Empty topic will remove, for a topic of the larder.
#[must_use]
pub fn empty_form(topic: &str) -> Option<EmptyTopicView> {
    let v = view();
    let r = v.topics.iter().find(|t| t.name == topic)?;
    let in_use = crate::model::InUse {
        groups: r
            .read_by
            .iter()
            .map(|g| (g.clone(), "Stable".to_owned(), 2))
            .collect(),
        transactions: Vec::new(),
    };
    Some(EmptyTopicView {
        topic: topic.to_owned(),
        partitions: usize::try_from(r.partitions).unwrap_or(0),
        bytes: r.bytes,
        records: i64::try_from(r.bytes / 180).unwrap_or(0),
        cleanup: r.cleanup,
        in_use: data_words::empty_in_use(&in_use),
        keeps: data_words::empty_keeps(r.cleanup != Cleanup::Delete),
        blocked: r.txn_open.then(|| {
            DataRefusal::TxnOpen {
                topic: Some(topic.to_owned()),
                id: Some("payments-tx-1".to_owned()),
            }
            .sentence()
        }),
    })
}

/// Empty topic in the larder: refused on a topic with a transaction under way, done on the others.
///
/// # Errors
/// No such topic, or a transaction under way.
pub fn empty_topic(topic: &str) -> Result<Emptied, DataRefusal> {
    let form = empty_form(topic).ok_or_else(|| DataRefusal::NoTopic {
        topic: topic.to_owned(),
    })?;
    if form.blocked.is_some() {
        return Err(DataRefusal::TxnOpen {
            topic: Some(topic.to_owned()),
            id: Some("payments-tx-1".to_owned()),
        });
    }
    Ok(Emptied {
        topic: topic.to_owned(),
        partitions: form.partitions,
        offsets: form.records,
        freed_bytes: form.bytes,
    })
}

/// Empty all topics in the larder.
#[must_use]
pub fn emptied_all() -> EmptiedAll {
    EmptiedAll {
        topics: usage()
            .topics
            .iter()
            .map(|t| Emptied {
                topic: t.name.clone(),
                partitions: usize::try_from(t.partitions).unwrap_or(0),
                offsets: i64::try_from(t.bytes / 180).unwrap_or(0),
                freed_bytes: t.bytes,
            })
            .collect(),
    }
}

/// Erase everything in the larder.
///
/// # Errors
/// Never: the larder has no transaction under way that the broker would refuse for.
pub fn erased() -> Result<Erased, DataRefusal> {
    let u = usage();
    Ok(Erased {
        topics: u.topics.len(),
        freed_bytes: u.topics_bytes() + u.stray_bytes(),
        groups: 4,
        groups_kept: vec!["search-indexer".to_owned()],
        transactional_ids: 2,
    })
}
