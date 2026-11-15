//! A share group's share-partitions, as the console shows them: where each starts and ends, how many of its in-flight
//! records are in each state, who holds what, how often records have been delivered, how many were archived. No request
//! says this (DescribeShareGroupOffsets tells the start and the lag only).
//!
//! A share-partition in memory is described as it is now; one not in memory (not fetched since the broker started) as it
//! was stored, with nothing held (a lock does not outlive the broker).

use std::collections::BTreeMap;

use kafka_auth::authz::{Operation, ResourceType};
use kafka_meta::{ShareRecordState, ShareState};
use kafka_storage::fs::Fs;
use kafka_wire::ids::GroupId;

use super::ConsoleView;
use crate::share::partition::{RecordState, SharePartition};
use crate::share::{Key, limits_of};
use crate::{Caller, blocking, clock};

/// One share-partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharePartitionView {
    pub topic: String,
    pub partition: i32,
    /// The share-partition start offset (SPSO); `None` before its first fetch (its start not set yet).
    pub start: Option<i64>,
    /// The share-partition end offset (SPEO): the last offset in flight, or the start when nothing is.
    pub end: Option<i64>,
    /// Whether it is in memory (fetched since the broker started), so the counts are live.
    pub live: bool,
    pub available: u64,
    pub acquired: u64,
    pub acknowledged: u64,
    pub archived: u64,
    /// Of the records in flight, how many have been delivered how many times (delivery count to records).
    pub deliveries: BTreeMap<i16, u64>,
    /// Who holds acquired records: member id to how many.
    pub holders: BTreeMap<String, u64>,
    /// When the soonest lock runs out, in milliseconds from the look.
    pub next_lock_ms: Option<i64>,
}

/// A share group's share-partitions and the limits they are held to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareGroupView {
    pub partitions: Vec<SharePartitionView>,
    pub delivery_limit: i64,
    pub lock_ms: i64,
    pub record_locks: i64,
    /// `share.auto.offset.reset` as the group has it.
    pub reset: String,
    pub read_committed: bool,
}

fn add(v: &mut SharePartitionView, state: RecordState, count: i16, holder: Option<&str>, n: u64) {
    match state {
        RecordState::Available => v.available += n,
        RecordState::Acquired => v.acquired += n,
        RecordState::Acknowledged => v.acknowledged += n,
        RecordState::Archived => v.archived += n,
    }
    if count > 0 {
        *v.deliveries.entry(count).or_default() += n;
    }
    if let Some(h) = holder {
        *v.holders.entry(h.to_owned()).or_default() += n;
    }
}

fn of_live(topic: &str, partition: i32, p: &SharePartition, now: i64) -> SharePartitionView {
    let mut v = blank(topic, partition);
    v.live = true;
    v.start = Some(p.start);
    v.end = Some(p.end);
    for (_, r) in p.records_from_start() {
        add(&mut v, r.state, r.count, r.holder.as_deref(), 1);
    }
    v.next_lock_ms = p.next_deadline().map(|at| at.saturating_sub(now));
    v
}

fn of_stored(topic: &str, partition: i32, s: &ShareState) -> SharePartitionView {
    let mut v = blank(topic, partition);
    if s.start_offset >= 0 {
        v.start = Some(s.start_offset);
        v.end = Some(s.batches.last().map_or(s.start_offset, |b| b.last));
    }
    for b in &s.batches {
        let n = u64::try_from(b.last - b.first + 1).unwrap_or(0);
        let state = match b.state {
            ShareRecordState::Available => RecordState::Available,
            ShareRecordState::Acknowledged => RecordState::Acknowledged,
            ShareRecordState::Archived => RecordState::Archived,
        };
        add(&mut v, state, b.delivery_count, None, n);
    }
    v
}

fn blank(topic: &str, partition: i32) -> SharePartitionView {
    SharePartitionView {
        topic: topic.to_owned(),
        partition,
        start: None,
        end: None,
        live: false,
        available: 0,
        acquired: 0,
        acknowledged: 0,
        archived: 0,
        deliveries: BTreeMap::new(),
        holders: BTreeMap::new(),
        next_lock_ms: None,
    }
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The share-partitions of `group`, if `caller` may DESCRIBE it; a topic the caller may not DESCRIBE is left out.
    pub fn share_group(&self, caller: &Caller, group: &str) -> Option<ShareGroupView> {
        let b = self.broker;
        if !b
            .authz
            .allows(caller, ResourceType::Group, group, Operation::Describe)
        {
            return None;
        }
        let id = GroupId::parse(group).ok()?;
        let stored = blocking::run(|| b.meta.share_states(&id)).ok()?;
        let now = clock::now().get();
        let partitions = stored
            .iter()
            .filter(|(t, _, _)| b.topic_allows(caller, t.as_str(), Operation::Describe))
            .filter_map(|(t, p, s)| {
                let topic = b.meta.topic(t)?;
                let key = Key {
                    group: id.clone(),
                    topic: topic.id.to_uuid(),
                    partition: p.get(),
                };
                Some(match b.shares.get(&key) {
                    Some(held) => of_live(t.as_str(), p.get(), &held.lock(), now),
                    None => of_stored(t.as_str(), p.get(), s),
                })
            })
            .collect();
        let s = b.share_settings(&id);
        let limits = limits_of(&s);
        Some(ShareGroupView {
            partitions,
            delivery_limit: i64::from(limits.max_delivery),
            lock_ms: limits.lock_ms,
            record_locks: limits.max_in_flight,
            reset: b
                .group_configs
                .of(&id)
                .get(crate::group_configs::RESET)
                .cloned()
                .unwrap_or_else(|| "latest".to_owned()),
            read_committed: s.read_committed,
        })
    }
}
