//! The producers one partition remembers, with the window of batches it keeps for each, and what the open transactions hold
//! back. The protocol (DescribeProducers) says the id, the epoch and the last sequence; the five batches behind it are what
//! the partition uses to recognise a batch sent again, and only the console asks to see them.
//!
//! Lazy and read-only: the writer is asked for a copy and nothing is kept.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ids::{PartitionId, TopicName};

use super::ConsoleView;
use crate::{Caller, clock};

/// A batch a producer wrote that the partition still remembers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProducerBatch {
    /// The sequence numbers of its first and last record.
    pub first_sequence: i32,
    pub last_sequence: i32,
    /// Where it went: the offset of its first record, and of its last.
    pub first_offset: i64,
    pub last_offset: i64,
    /// Its newest timestamp as stored.
    pub at_ms: i64,
}

/// One producer on the partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerWindow {
    pub id: i64,
    pub epoch: i16,
    /// The last sequence the partition accepted, if it knows (a producer known only by a marker has none).
    pub last_sequence: Option<i32>,
    /// When it last wrote, in milliseconds since the epoch.
    pub last_used_ms: i64,
    /// The last batches, newest first (at most five).
    pub batches: Vec<ProducerBatch>,
    /// The first offset of its open transaction on this partition, if it has one.
    pub open_from: Option<i64>,
}

/// A transaction that was aborted and that the partition still remembers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbortedRange {
    pub producer: i64,
    /// Its first record, and its abort marker.
    pub first: i64,
    pub marker: i64,
}

/// One partition's producers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionProducers {
    pub topic: String,
    pub partition: i32,
    pub now_ms: i64,
    /// The log start and end, and the last stable offset.
    pub start: i64,
    pub end: i64,
    pub last_stable: i64,
    /// Newest write first.
    pub producers: Vec<ProducerWindow>,
    /// The aborted transactions remembered, oldest first.
    pub aborted: Vec<AbortedRange>,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The producers of `partition` of `topic`, if `caller` may READ the topic (as DescribeProducers asks) and the partition
    /// exists; `None` otherwise (the two are not told apart, as Kafka does not).
    pub async fn producers(
        &self,
        caller: &Caller,
        topic: &str,
        partition: i32,
    ) -> Option<PartitionProducers> {
        let b = self.broker;
        if !b.topic_allows(caller, topic, operation::READ) {
            return None;
        }
        let found = b.meta.topic(&TopicName::parse(topic).ok()?)?;
        let id = PartitionId::new(partition).filter(|p| p.get() < found.partitions.get())?;
        let was_open = b.partitions.is_open(found.id, id);
        // A partition never written is not opened: opening makes its first segment, and a read must write nothing.
        let Some(open) = b.partitions.get_unless_empty(&found, id).await.ok()? else {
            return Some(PartitionProducers {
                topic: topic.to_owned(),
                partition,
                now_ms: clock::now().get(),
                start: 0,
                end: 0,
                last_stable: 0,
                producers: Vec::new(),
                aborted: Vec::new(),
            });
        };
        let seen = open.txns().await.ok();
        let windows = open.windows().await.ok();
        let stable = open.stability();
        let reader = open.reader();
        let (start, end) = (reader.start_offset().get(), reader.end_offset().get());
        drop(open);
        if !was_open {
            b.partitions.release_if_idle(found.id, id).await;
        }
        let (seen, windows) = (seen?, windows?);
        let mut producers: Vec<ProducerWindow> = windows
            .into_iter()
            .map(|w| ProducerWindow {
                id: w.id,
                epoch: w.epoch,
                last_sequence: w.recent.last().map(|s| s.last),
                last_used_ms: w.last_used_ms,
                open_from: seen
                    .open
                    .iter()
                    .find(|o| o.producer == w.id)
                    .map(|o| o.first),
                batches: w
                    .recent
                    .iter()
                    .rev()
                    .map(|s| ProducerBatch {
                        first_sequence: s.first,
                        last_sequence: s.last,
                        first_offset: s.base_offset,
                        last_offset: s.base_offset
                            + i64::from(s.last.wrapping_sub(s.first) & i32::MAX),
                        at_ms: s.max_timestamp,
                    })
                    .collect(),
            })
            .collect();
        producers.sort_by_key(|p| (std::cmp::Reverse(p.last_used_ms), p.id));
        Some(PartitionProducers {
            topic: topic.to_owned(),
            partition,
            now_ms: clock::now().get(),
            start,
            end,
            last_stable: stable.last_stable.get(),
            producers,
            aborted: stable
                .aborted
                .iter()
                .map(|a| AbortedRange {
                    producer: a.producer,
                    first: a.first,
                    marker: a.last,
                })
                .collect(),
        })
    }
}
