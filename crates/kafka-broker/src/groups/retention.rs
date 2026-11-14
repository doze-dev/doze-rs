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
        // A consumer group of the new protocol without members keeps each offset for the retention from its own commit,
        // not from when its last member left; once the members it had cannot be there any more.
        (members, Some(life)) if life.consumer.is_some() => {
            let waiting = life.live
                && match members {
                    Members::NotInMemory => {
                        started_ms.saturating_add(i64::from(life.session_ms)) > now_ms
                    }
                    _ => true,
                };
            if waiting { Vec::new() } else { own_age(None) }
        }
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

/// What a group's description says of its members, in the terms the rules above judge by.
pub(crate) fn members_of(described: Option<&super::Description>) -> Members<'_> {
    match described {
        // A consumer group of the new protocol with members: not in a rebalance as far as its offsets go.
        Some(d) if d.consumer.as_ref().is_some_and(|c| !c.members.is_empty()) => Members::Running {
            rebalancing: false,
            consumer: true,
            subscribed: d.subscribed.as_ref(),
        },
        Some(d) if !d.members.is_empty() => Members::Running {
            rebalancing: matches!(
                d.state,
                super::State::PreparingRebalance | super::State::CompletingRebalance
            ),
            consumer: d.protocol_type.as_deref() == Some("consumer"),
            subscribed: d.subscribed.as_ref(),
        },
        Some(_) => Members::Empty,
        None => Members::NotInMemory,
    }
}

#[cfg(test)]
mod tests;
