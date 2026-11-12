//! The view-model of a topic's Consumers tab: the consumer groups that have committed offsets on the topic, each with its lag
//! on this topic's partitions. Names start with `TopicReader` or `Consumers` so that no other family can clash.

use super::groups::{GroupPhase, GroupProtocol, LagTopic};
use super::topic::TopicHead;

/// One consumer group as it reads this topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicReader {
    pub id: String,
    pub protocol: GroupProtocol,
    pub phase: GroupPhase,
    /// How many members the group has (all of them, not only those that hold this topic).
    pub members: usize,
    /// Of the members, how many hold at least one partition of this topic.
    pub holding: usize,
    /// The group's commits on this topic, a row per partition, and the lag they add up to.
    pub block: LagTopic,
    pub last_commit_ms: Option<i64>,
}

/// The Consumers tab of a topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumersView {
    pub head: TopicHead,
    pub readers: Vec<TopicReader>,
    /// How many groups have committed on the topic, before the cap of what is shown.
    pub total: usize,
}
