//! Which committed offsets have outlived `offsets.retention`, by the rules a real Kafka applies (recorded in
//! `offsets-retention`, `offsets-retention-restart` and `offsets-retention-recreate`):
//!
//! - A classic group with no members, one that had some, keeps its offsets for the retention from the moment it became
//!   empty, however late the last commit was: a commit into an empty group does not extend it.
//! - A group that never had a member (a standalone consumer committing by hand) keeps each offset for the retention
//!   from its own commit.
//! - A group with members expires nothing for the topics it is subscribed to. The offsets of a topic it is not
//!   subscribed to expire from their own commit. A group in the middle of a rebalance is not looked at at all, nor is a
//!   group whose protocol type is not `consumer` or whose members' subscriptions cannot be read.
//! - A restart changes none of this. A real broker has its members again when it is back, and a member that does not
//!   return runs out of session: the group is without members from then, not from the restart. This broker does not
//!   keep members, so a group recorded as having them is taken to have them until the longest session has run out
//!   after the start.
//!
//! Whatever is not known for certain keeps the offset.

use std::collections::BTreeSet;

use kafka_meta::{GroupLife, GroupOffset};
use kafka_wire::ids::{PartitionId, TopicName};

/// An offset that has expired: where it is, and the time it was committed at when it was seen (so that one committed
/// again since is not taken).
pub(crate) type Expired = (TopicName, PartitionId, i64);

/// What is known of a group's members right now.
pub(crate) enum Members<'a> {
    /// The group is running with members.
    Running {
        /// Members are joining again or waiting for the leader's assignment.
        rebalancing: bool,
        /// The members' protocol type is `consumer`.
        consumer: bool,
        /// The topics they subscribe to, if every subscription could be read.
        subscribed: Option<&'a BTreeSet<String>>,
    },
    /// The group is in memory, without members.
    Empty,
    /// The group is not in memory (the broker has started since it was last seen).
    NotInMemory,
}

/// Everything the judgement needs about one group.
pub(crate) struct View<'a> {
    pub(crate) members: Members<'a>,
    /// What was recorded of its members.
    pub(crate) life: Option<&'a GroupLife>,
    /// What it has committed.
    pub(crate) offsets: &'a [GroupOffset],
}

/// The partitions whose offsets have expired.
pub(crate) fn expired(
    view: &View<'_>,
    now_ms: i64,
    retention_ms: i64,
    started_ms: i64,
) -> Vec<Expired> {
    if retention_ms <= 0 {
        return Vec::new();
    }
    let cutoff = now_ms.saturating_sub(retention_ms);
    let own_age = |subscribed: Option<&BTreeSet<String>>| {
        view.offsets
            .iter()
            .filter(|o| o.offset.committed_ms < cutoff)
            .filter(|o| subscribed.is_none_or(|s| !s.contains(o.topic.as_str())))
            .map(|o| (o.topic.clone(), o.partition, o.offset.committed_ms))
            .collect::<Vec<_>>()
    };
    let all = || {
        view.offsets
            .iter()
            .map(|o| (o.topic.clone(), o.partition, o.offset.committed_ms))
            .collect::<Vec<_>>()
    };
    match (&view.members, view.life) {
        (
            Members::Running {
                rebalancing,
                consumer,
                subscribed,
            },
            _,
        ) => match subscribed {
            Some(topics) if *consumer && !rebalancing => own_age(Some(topics)),
            _ => Vec::new(),
        },
        // Nobody ever joined: each offset is its own.
        (_, None) => own_age(None),
        (members, Some(life)) => {
            let since = if life.live {
                match members {
                    // Recorded as having members, and none has come back: they are taken to be there until their
                    // longest session has run out, and gone from then.
                    Members::NotInMemory => {
                        Some(started_ms.saturating_add(i64::from(life.session_ms)))
                    }
                    // In memory without any, while the record still says it has them: the record is a write behind.
                    _ => None,
                }
            } else {
                life.empty_since_ms
            };
            match since {
                Some(since) if since < cutoff => all(),
                Some(_) => Vec::new(),
                None if life.live => Vec::new(),
                // Emptied with no time recorded: as good as never having had a member.
                None => own_age(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kafka_meta::StoredOffset;

    use super::*;

    const DAY: i64 = 86_400_000;

    fn offset(topic: &str, partition: i32, committed_ms: i64) -> GroupOffset {
        GroupOffset {
            topic: TopicName::parse(topic).unwrap(),
            partition: PartitionId::new(partition).unwrap(),
            offset: StoredOffset {
                offset: 1,
                leader_epoch: -1,
                metadata: String::new(),
                committed_ms,
            },
        }
    }

    fn life(live: bool, session_ms: u32, empty_since_ms: Option<i64>) -> GroupLife {
        GroupLife {
            live,
            session_ms,
            empty_since_ms,
            protocol_type: "consumer".into(),
        }
    }

    fn names(list: Vec<Expired>) -> Vec<String> {
        let mut out: Vec<_> = list.iter().map(|(t, p, _)| format!("{t}-{p}")).collect();
        out.sort();
        out
    }

    /// A week's retention; now is day 10; the broker started at day 9.
    fn run(view: &View<'_>) -> Vec<String> {
        names(expired(view, 10 * DAY, 7 * DAY, 9 * DAY))
    }

    #[test]
    fn a_group_that_never_had_a_member_keeps_each_offset_for_the_retention_from_its_own_commit() {
        let offsets = [offset("t", 0, 2 * DAY), offset("t", 1, 4 * DAY)];
        for members in [Members::Empty, Members::NotInMemory] {
            let view = View {
                members,
                life: None,
                offsets: &offsets,
            };
            assert_eq!(run(&view), ["t-0"], "day 2 is eight days old, day 4 is six");
        }
    }

    #[test]
    fn an_emptied_group_keeps_its_offsets_for_the_retention_from_when_it_became_empty_however_late_the_commit()
     {
        // Empty since day 5 (five days ago): kept, even though the offset is eight days old.
        let offsets = [offset("t", 0, 2 * DAY)];
        let l = life(false, 10_000, Some(5 * DAY));
        let view = View {
            members: Members::Empty,
            life: Some(&l),
            offsets: &offsets,
        };
        assert!(run(&view).is_empty());
        // Empty since day 2: all go, even an offset committed yesterday (a commit into an empty group does not extend).
        let offsets = [offset("t", 0, 2 * DAY), offset("t", 1, 9 * DAY)];
        let l = life(false, 10_000, Some(2 * DAY));
        let view = View {
            members: Members::Empty,
            life: Some(&l),
            offsets: &offsets,
        };
        assert_eq!(run(&view), ["t-0", "t-1"]);
    }

    #[test]
    fn a_group_with_members_expires_nothing_for_the_topics_it_subscribes_to_and_the_rest_by_their_own_age()
     {
        let subscribed: BTreeSet<String> = ["t".to_owned()].into();
        let offsets = [
            offset("t", 0, DAY),
            offset("u", 0, DAY),
            offset("u", 1, 9 * DAY),
        ];
        let view = View {
            members: Members::Running {
                rebalancing: false,
                consumer: true,
                subscribed: Some(&subscribed),
            },
            life: None,
            offsets: &offsets,
        };
        assert_eq!(run(&view), ["u-0"]);
    }

    #[test]
    fn a_group_in_a_rebalance_or_of_another_kind_or_with_an_unreadable_subscription_is_not_looked_at()
     {
        let subscribed: BTreeSet<String> = BTreeSet::new();
        let offsets = [offset("u", 0, DAY)];
        let looked = |rebalancing, consumer, subscribed: Option<&BTreeSet<String>>| {
            run(&View {
                members: Members::Running {
                    rebalancing,
                    consumer,
                    subscribed,
                },
                life: None,
                offsets: &offsets,
            })
        };
        assert_eq!(
            looked(false, true, Some(&subscribed)),
            ["u-0"],
            "the control: looked at"
        );
        assert!(looked(true, true, Some(&subscribed)).is_empty());
        assert!(looked(false, false, Some(&subscribed)).is_empty());
        assert!(looked(false, true, None).is_empty());
    }

    #[test]
    fn a_group_recorded_as_having_members_that_nobody_has_rejoined_is_presumed_to_have_them_until_their_session_ends()
     {
        let offsets = [offset("t", 0, DAY)];
        let ran = |started_ms: i64, session_ms: u32| {
            let l = life(true, session_ms, None);
            let view = View {
                members: Members::NotInMemory,
                life: Some(&l),
                offsets: &offsets,
            };
            names(expired(&view, 10 * DAY, 7 * DAY, started_ms))
        };
        // Started yesterday: the members are taken to have left only a minute after that, which is not a week ago.
        assert!(ran(9 * DAY, 60_000).is_empty());
        // Started eight days ago: gone from a minute after that, which is more than a week ago.
        assert_eq!(ran(2 * DAY, 60_000), ["t-0"]);
        // The same start with a session of nearly fifty days: still presumed to be there.
        assert!(ran(2 * DAY, u32::MAX).is_empty());
    }

    #[test]
    fn a_record_that_says_members_for_a_group_in_memory_without_any_keeps_the_offsets() {
        let offsets = [offset("t", 0, DAY)];
        let l = life(true, 10_000, None);
        let view = View {
            members: Members::Empty,
            life: Some(&l),
            offsets: &offsets,
        };
        assert!(run(&view).is_empty(), "unknown keeps");
    }

    #[test]
    fn a_retention_of_zero_keeps_everything() {
        let offsets = [offset("t", 0, 0)];
        let view = View {
            members: Members::Empty,
            life: None,
            offsets: &offsets,
        };
        assert!(expired(&view, 10 * DAY, 0, 9 * DAY).is_empty());
    }
}
