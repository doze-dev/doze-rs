//! The server-side assignors: given who subscribes to what, decide what each member is to own. The group
//! (`../consumer.rs`) then moves its members towards that, one safe step at a time.
//!
//! Both are pure. `uniform` spreads every partition of every topic over the members subscribed to it, as evenly as
//! the subscriptions allow, and keeps a member's partitions where it can, so that a member joining or leaving moves
//! as little as it must. `range` gives each subscriber of a topic a contiguous run of its partitions, topic by topic,
//! so that topics with as many partitions line up (co-partitioning).

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

/// One partition of one topic, by the topic's id: a topic deleted and made again is another topic.
pub(crate) type Partition = (Uuid, i32);
/// A set of partitions.
pub(crate) type Partitions = BTreeSet<Partition>;

/// A topic as the assignors see it.
#[derive(Debug, Clone)]
pub(crate) struct Subscribed {
    pub(crate) id: Uuid,
    pub(crate) partitions: i32,
    /// Who subscribes to it, in order.
    pub(crate) subscribers: Vec<MemberId>,
}

/// Which assignor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Assignor {
    #[default]
    Uniform,
    Range,
}

impl Assignor {
    /// Kafka's name for it.
    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Range => "range",
        }
    }

    #[must_use]
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "uniform" => Some(Self::Uniform),
            "range" => Some(Self::Range),
            _ => None,
        }
    }
}

/// What each of `members` is to own. `topics` are in the order to deal them (by name); `previous` is what each owned
/// under the last assignment, which `uniform` keeps. Every member has an entry, empty if it gets nothing.
#[must_use]
pub(crate) fn assign(
    assignor: Assignor,
    members: &[MemberId],
    topics: &[Subscribed],
    previous: &BTreeMap<MemberId, Partitions>,
) -> BTreeMap<MemberId, Partitions> {
    let mut targets: BTreeMap<MemberId, Partitions> = members
        .iter()
        .map(|m| (m.clone(), Partitions::new()))
        .collect();
    match assignor {
        Assignor::Range => range(topics, &mut targets),
        Assignor::Uniform => uniform(topics, previous, &mut targets),
    }
    targets
}

fn range(topics: &[Subscribed], targets: &mut BTreeMap<MemberId, Partitions>) {
    for topic in topics {
        let Ok(k) = i32::try_from(topic.subscribers.len()) else {
            continue;
        };
        if k == 0 {
            continue;
        }
        let (base, extra) = (topic.partitions / k, topic.partitions % k);
        let mut start = 0_i32;
        for (i, member) in (0_i32..).zip(&topic.subscribers) {
            let count = base + i32::from(i < extra);
            if let Some(mine) = targets.get_mut(member) {
                mine.extend((start..start + count).map(|p| (topic.id, p)));
            }
            start += count;
        }
    }
}

/// A partition and who may have it.
struct Seat<'a> {
    partition: Partition,
    eligible: &'a [MemberId],
}

fn uniform(
    topics: &[Subscribed],
    previous: &BTreeMap<MemberId, Partitions>,
    targets: &mut BTreeMap<MemberId, Partitions>,
) {
    let seats: Vec<Seat> = topics
        .iter()
        .flat_map(|t| {
            (0..t.partitions).map(|p| Seat {
                partition: (t.id, p),
                eligible: &t.subscribers,
            })
        })
        .collect();
    let by_partition: BTreeMap<Partition, &[MemberId]> =
        seats.iter().map(|s| (s.partition, s.eligible)).collect();
    let mut owner: BTreeMap<Partition, MemberId> = BTreeMap::new();
    let mut count: BTreeMap<MemberId, usize> = targets.keys().map(|m| (m.clone(), 0)).collect();

    // Sticky: keep what each member had, as long as the partition is still there and the member still wants its topic.
    for (member, held) in previous.iter().filter(|(m, _)| targets.contains_key(*m)) {
        for part in held {
            let wanted = by_partition
                .get(part)
                .is_some_and(|subs| subs.binary_search(member).is_ok());
            if wanted && !owner.contains_key(part) {
                owner.insert(*part, member.clone());
                *count.entry(member.clone()).or_default() += 1;
            }
        }
    }
    // The rest go to whoever eligible has the fewest.
    for seat in &seats {
        if !owner.contains_key(&seat.partition)
            && let Some(to) = least(seat.eligible, &count)
        {
            owner.insert(seat.partition, to.clone());
            *count.entry(to).or_default() += 1;
        }
    }
    // Even out: move a partition to an eligible member that has at least two fewer, until none can.
    let mut moved = true;
    while moved {
        moved = false;
        for seat in seats.iter().rev() {
            let (Some(from), Some(to)) = (
                owner.get(&seat.partition).cloned(),
                least(seat.eligible, &count),
            ) else {
                continue;
            };
            let (have, would) = (
                count.get(&from).copied().unwrap_or(0),
                count.get(&to).copied().unwrap_or(0),
            );
            if have >= would + 2 {
                owner.insert(seat.partition, to.clone());
                count.insert(from, have - 1);
                count.insert(to, would + 1);
                moved = true;
            }
        }
    }
    for (partition, member) in owner {
        if let Some(mine) = targets.get_mut(&member) {
            mine.insert(partition);
        }
    }
}

/// The first of `eligible` with the fewest partitions so far.
fn least(eligible: &[MemberId], count: &BTreeMap<MemberId, usize>) -> Option<MemberId> {
    let mut best: Option<(&MemberId, usize)> = None;
    for member in eligible {
        let n = count.get(member).copied().unwrap_or(0);
        if best.is_none_or(|(_, b)| n < b) {
            best = Some((member, n));
        }
    }
    best.map(|(m, _)| m.clone())
}

#[cfg(test)]
mod tests;
