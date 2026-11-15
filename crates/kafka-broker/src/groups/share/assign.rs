//! The simple assignor (Kafka's `SimpleAssignor`, the only one share groups have): every partition of the topics members
//! subscribe to is given to at least one of them, and a partition is shared when there are more members than partitions.
//! A member keeps what it had where it can.
//!
//! Kafka walks hash sets, so which member gets the larger share when they do not divide evenly is an accident of hashing
//! there; here members are taken in the order of their ids, and partitions in the order of topic name and index.

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

use crate::groups::consumer::assign::{Partition, Partitions};

/// A topic the members subscribe to: its id, its partitions and who subscribes.
#[derive(Debug, Clone)]
pub(crate) struct Subscribed {
    pub(crate) id: Uuid,
    pub(crate) partitions: i32,
    pub(crate) subscribers: BTreeSet<MemberId>,
}

/// What each member is to hold. `topics` by name; `previous` is what each held under the last assignment. Every member
/// has an entry. When all members subscribe to the same topics they are assigned over all the partitions at once (Kafka's
/// homogeneous builder); otherwise topic by topic, over that topic's subscribers (the heterogeneous one).
#[must_use]
pub(crate) fn assign(
    members: &BTreeSet<MemberId>,
    topics: &BTreeMap<String, Subscribed>,
    previous: &BTreeMap<MemberId, Partitions>,
) -> BTreeMap<MemberId, Partitions> {
    let mut out: BTreeMap<MemberId, Partitions> =
        members.iter().map(|m| (m.clone(), Partitions::new())).collect();
    let homogeneous = topics.values().all(|t| t.subscribers == *members);
    if homogeneous {
        let ids: Vec<MemberId> = members.iter().cloned().collect();
        let partitions: Vec<Partition> = topics.values().flat_map(partitions_of).collect();
        place(&ids, &partitions, previous, &mut out);
    } else {
        for t in topics.values() {
            let ids: Vec<MemberId> = t.subscribers.iter().filter(|m| members.contains(*m)).cloned().collect();
            let partitions: Vec<Partition> = partitions_of(t).collect();
            let previous: BTreeMap<MemberId, Partitions> = previous
                .iter()
                .map(|(m, ps)| (m.clone(), ps.iter().filter(|p| p.0 == t.id).copied().collect()))
                .collect();
            let mut placed: BTreeMap<MemberId, Partitions> = BTreeMap::new();
            place(&ids, &partitions, &previous, &mut placed);
            for (m, ps) in placed {
                out.entry(m).or_default().extend(ps);
            }
        }
    }
    out
}

fn partitions_of(t: &Subscribed) -> impl Iterator<Item = Partition> + '_ {
    (0..t.partitions.max(0)).map(move |p| (t.id, p))
}

/// Kafka's `SimpleHomogeneousAssignmentBuilder`, over `members` and `partitions`: each partition goes to `ceil(M/P)`
/// members, and member `i` gets `ceil(x(i+1)) - ceil(xi)` of them where `x = sharing * P / M`.
fn place(
    members: &[MemberId],
    partitions: &[Partition],
    previous: &BTreeMap<MemberId, Partitions>,
    out: &mut BTreeMap<MemberId, Partitions>,
) {
    let (m, p) = (members.len(), partitions.len());
    for id in members {
        out.entry(id.clone()).or_default();
    }
    if m == 0 || p == 0 {
        return;
    }
    let sharing = m.div_ceil(p);
    // ceil(sharing * p * i / m), in integers.
    let edge = |i: usize| (sharing * p * i).div_ceil(m);
    let desired: Vec<usize> = (0..m).map(|i| edge(i + 1) - edge(i)).collect();
    let target: BTreeSet<Partition> = partitions.iter().copied().collect();
    let mut by_partition: BTreeMap<Partition, BTreeSet<usize>> =
        partitions.iter().map(|p| (*p, BTreeSet::new())).collect();
    let mut by_member: Vec<Vec<Partition>> = vec![Vec::new(); m];
    // What each still holds of what it had, as long as the partition is still to be assigned.
    for (i, id) in members.iter().enumerate() {
        for p in previous.get(id).into_iter().flatten() {
            if target.contains(p) {
                by_member[i].push(*p);
                by_partition.entry(*p).or_default().insert(i);
            }
        }
    }
    // An over-filled member gives up the excess.
    for (i, held) in by_member.iter_mut().enumerate() {
        while held.len() > desired[i] {
            if let Some(p) = held.pop() {
                by_partition.entry(p).or_default().remove(&i);
            }
        }
    }
    // A partition held by more members than it is to be shared by loses the excess.
    for (p, holders) in &mut by_partition {
        while holders.len() > sharing {
            let Some(i) = holders.iter().next_back().copied() else {
                break;
            };
            holders.remove(&i);
            by_member[i].retain(|q| q != p);
        }
    }
    // The rest, cycling through the members with room, each partition up to its sharing.
    let mut unfilled: Vec<usize> = (0..m).filter(|i| by_member[*i].len() < desired[*i]).collect();
    let mut cursor = 0;
    for (p, holders) in &mut by_partition {
        let mut placed_this_cycle = false;
        let mut tried = 0;
        while holders.len() < sharing && !unfilled.is_empty() {
            if cursor >= unfilled.len() {
                if !placed_this_cycle && tried > 0 {
                    break;
                }
                cursor = 0;
                placed_this_cycle = false;
                tried = 0;
            }
            let i = unfilled[cursor];
            tried += 1;
            if holders.contains(&i) {
                cursor += 1;
                continue;
            }
            holders.insert(i);
            by_member[i].push(*p);
            placed_this_cycle = true;
            if by_member[i].len() >= desired[i] {
                unfilled.remove(cursor);
            } else {
                cursor += 1;
            }
        }
        if unfilled.is_empty() {
            break;
        }
    }
    for (i, id) in members.iter().enumerate() {
        out.entry(id.clone()).or_default().extend(by_member[i].iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Uuid = Uuid::from_u128(1);
    const U: Uuid = Uuid::from_u128(2);

    fn id(s: &str) -> MemberId {
        MemberId::new(s).unwrap()
    }

    fn members(ids: &[&str]) -> BTreeSet<MemberId> {
        ids.iter().map(|s| id(s)).collect()
    }

    fn topic(uuid: Uuid, partitions: i32, subs: &[&str]) -> Subscribed {
        Subscribed {
            id: uuid,
            partitions,
            subscribers: members(subs),
        }
    }

    fn counts(out: &BTreeMap<MemberId, Partitions>) -> Vec<usize> {
        out.values().map(BTreeSet::len).collect()
    }

    #[test]
    fn partitions_are_spread_and_shared_when_there_are_more_members() {
        let all = members(&["a", "b"]);
        let topics = BTreeMap::from([("t".to_owned(), topic(T, 3, &["a", "b"]))]);
        let out = assign(&all, &topics, &BTreeMap::new());
        assert_eq!(counts(&out), [2, 1], "P=3, M=2");
        let all = members(&["a", "b", "c"]);
        let topics = BTreeMap::from([("t".to_owned(), topic(T, 1, &["a", "b", "c"]))]);
        let out = assign(&all, &topics, &BTreeMap::new());
        assert!(out.values().all(|ps| *ps == Partitions::from([(T, 0)])), "all share the one partition");
        let topics = BTreeMap::from([("t".to_owned(), topic(T, 2, &["a", "b", "c"]))]);
        let out = assign(&all, &topics, &BTreeMap::new());
        assert_eq!(counts(&out), [2, 1, 1], "P=2, M=3: each partition shared by two");
        let mut sharing: BTreeMap<Partition, usize> = BTreeMap::new();
        for p in out.values().flatten() {
            *sharing.entry(*p).or_default() += 1;
        }
        assert_eq!(sharing.values().copied().collect::<Vec<_>>(), [2, 2]);
    }

    #[test]
    fn every_partition_has_a_member_and_a_member_keeps_what_it_had() {
        let all = members(&["a", "b", "c"]);
        let topics = BTreeMap::from([("t".to_owned(), topic(T, 6, &["a", "b", "c"]))]);
        let previous = BTreeMap::from([(id("c"), Partitions::from([(T, 0), (T, 1)]))]);
        let out = assign(&all, &topics, &previous);
        assert_eq!(counts(&out), [2, 2, 2]);
        assert_eq!(out[&id("c")], Partitions::from([(T, 0), (T, 1)]), "sticky");
        let union: Partitions = out.values().flatten().copied().collect();
        assert_eq!(union.len(), 6);
    }

    #[test]
    fn members_of_different_topics_are_assigned_topic_by_topic() {
        let all = members(&["a", "b"]);
        let topics = BTreeMap::from([
            ("t".to_owned(), topic(T, 2, &["a", "b"])),
            ("u".to_owned(), topic(U, 1, &["b"])),
        ]);
        let out = assign(&all, &topics, &BTreeMap::new());
        assert_eq!(out[&id("a")], Partitions::from([(T, 0)]));
        assert_eq!(out[&id("b")], Partitions::from([(T, 1), (U, 0)]));
    }

    #[test]
    fn nobody_and_nothing_assign_nothing() {
        let topics = BTreeMap::from([("t".to_owned(), topic(T, 0, &["a"]))]);
        let out = assign(&members(&["a"]), &topics, &BTreeMap::new());
        assert!(out[&id("a")].is_empty());
        assert!(assign(&BTreeSet::new(), &BTreeMap::new(), &BTreeMap::new()).is_empty());
    }
}
