//! What a group machine says of itself besides what it does: why a rebalance began, who it was about, what came of it.
//!
//! A note is an extra effect (`Effect::Note`), pushed beside the effects a step already returns and never in place of
//! one: no machine reads a note, none decides anything by one, and a run of the machine with the notes taken out is the
//! run of the machine before they existed (a seeded differential test holds it to that). The actor of the group keeps
//! them in a small ring (`events`) for the console; a build without the console says nothing at all.

use std::fmt;

use kafka_wire::ids::MemberId;

use super::machine::{Effect, Millis};

/// Whether the machines say what they do: only a build with the console (feature `inspect`) has anywhere to put it.
pub(crate) const CAPTURE: bool = cfg!(any(feature = "inspect", test));

/// The longest a client's own reason is kept, in bytes. A client chooses it, and the Go edition once held 1.8 GiB for a
/// 900 KiB one.
pub(crate) const REASON_BYTES: usize = 128;
/// The longest a client id is kept in a note, in bytes.
const CLIENT_BYTES: usize = 64;
/// The most members a note names; the rest are counted.
pub(crate) const MEMBERS_KEPT: usize = 8;
/// The most members, topics and partitions of an assignment a note keeps.
const HOLDS_MEMBERS: usize = 8;
const HOLDS_TOPICS: usize = 6;
const HOLDS_PARTITIONS: usize = 12;

/// Why a group began a new generation (classic) or a new epoch (the new protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cause {
    /// A member that was not in the group joined it, or the first of an empty group.
    MemberJoined,
    /// A member said it was going (LeaveGroup, or a heartbeat of epoch -1).
    MemberLeft,
    /// A member sent no heartbeat within its session timeout.
    SessionExpired,
    /// Partitions were added to, or a topic made or deleted among, the topics the members read (the new protocol: the
    /// broker is told; a classic group learns it only as its leader joining again).
    TopicChanged,
    /// A member that was in the group joined again offering other protocols than before.
    ProtocolsChanged,
    /// The leader of a running group joined again with the same protocols: Kafka starts a rebalance for that.
    LeaderRejoined,
    /// Members did not join again, or give up what they were told to, in time.
    RebalanceTimeout,
    /// A member changed what it asks for: its topics, its pattern or its assignor.
    SubscriptionChanged,
    /// A right to describe a topic was given or taken from a member's principal.
    RightsChanged,
    /// The last member of the new protocol left a group that still had classic members: it is a classic group again.
    BecameClassic,
    /// A member of the new protocol joined a classic group of consumers: it is a consumer group now.
    BecameConsumer,
}

/// A client's own words for why it joined or left, clipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reason(Box<str>);

impl Reason {
    /// The first [`REASON_BYTES`] of `text` at a character boundary, control characters left out, or `None` if nothing
    /// is left. Reads no more of `text` than it keeps, so a very long one costs no more than a short one.
    pub(crate) fn clip(text: &str) -> Option<Self> {
        let kept = clip(text, REASON_BYTES);
        (!kept.is_empty()).then(|| Self(kept.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn clip(text: &str, most: usize) -> String {
    let mut kept = String::new();
    for c in text.chars().filter(|c| !c.is_control()) {
        if kept.len() + c.len_utf8() > most {
            break;
        }
        kept.push(c);
    }
    kept
}

/// The member a note is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Named {
    pub(crate) member: MemberId,
    /// The `client.id` it joined with, clipped.
    pub(crate) client: Box<str>,
    /// What it said when it joined or left, if it said.
    pub(crate) reason: Option<Reason>,
}

impl Named {
    pub(crate) fn new(member: &MemberId, client: &str, reason: Option<Reason>) -> Self {
        Self {
            member: member.clone(),
            client: clip(client, CLIENT_BYTES).into(),
            reason,
        }
    }
}

/// Why a rebalance began, for the note that says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Why {
    pub(crate) cause: Cause,
    pub(crate) who: Option<Named>,
    /// More members than `who` did the same in one request.
    pub(crate) more: u32,
    /// How many members the group had before.
    pub(crate) before: u32,
}

impl Why {
    pub(crate) fn new(cause: Cause, who: Option<Named>, before: usize) -> Self {
        Self {
            cause,
            who,
            more: 0,
            before: count(before),
        }
    }
}

pub(crate) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// What a member holds in a generation, by topic, clipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hold {
    pub(crate) member: MemberId,
    pub(crate) topics: Vec<(Box<str>, Vec<i32>)>,
    /// How many partitions it holds in all.
    pub(crate) total: u32,
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum What {
    /// A rebalance began: members have until `wait_ms` to join again. `generation` is the one it will make.
    Began {
        why: Why,
        generation: i32,
        after: u32,
        wait_ms: u64,
    },
    /// The wait ended and the generation was formed: the members that joined, and the one that leads.
    Formed {
        generation: i32,
        members: Vec<MemberId>,
        total: u32,
        leader: Option<MemberId>,
    },
    /// A member did not join again in time and was dropped as the generation formed.
    Dropped {
        generation: i32,
        who: Named,
        wait_ms: u64,
    },
    /// The leader's assignment was handed out: the generation is running.
    Settled {
        generation: i32,
        holds: Vec<Hold>,
        total: u32,
    },
    /// A consumer group's epoch moved.
    Epoch { why: Why, epoch: i32, after: u32 },
}

/// One thing a machine says, and when on its clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) at: Millis,
    pub(crate) what: What,
}

impl Note {
    /// About what the note costs to keep, in bytes: what the ring holds to its budget.
    pub(crate) fn weight(&self) -> usize {
        let id = |m: &MemberId| m.as_str().len();
        let who =
            |w: &Named| id(&w.member) + w.client.len() + w.reason.as_ref().map_or(0, |r| r.0.len());
        let why = |w: &Why| w.who.as_ref().map_or(0, who);
        let extra = match &self.what {
            What::Began { why: w, .. } | What::Epoch { why: w, .. } => why(w),
            What::Formed {
                members, leader, ..
            } => members.iter().map(id).sum::<usize>() + leader.as_ref().map_or(0, id),
            What::Dropped { who: w, .. } => who(w),
            What::Settled { holds, .. } => holds
                .iter()
                .map(|h| {
                    id(&h.member)
                        + h.topics
                            .iter()
                            .map(|(t, ps)| t.len() + 4 * ps.len())
                            .sum::<usize>()
                })
                .sum(),
        };
        std::mem::size_of::<Self>() + extra
    }
}

/// Pushes a note beside the effects of a step, if the machine says what it does.
pub(crate) fn say(out: &mut Vec<Effect>, now: Millis, what: impl FnOnce() -> What) {
    if CAPTURE {
        out.push(Effect::Note(Note {
            at: now,
            what: what(),
        }));
    }
}

/// The assignment of each member as a [`Hold`], clipped; `members` are `(id, topics by name)`.
pub(crate) fn holds(
    members: impl Iterator<Item = (MemberId, std::collections::BTreeMap<String, Vec<i32>>)>,
) -> (Vec<Hold>, u32) {
    let mut kept = Vec::new();
    let mut total = 0_usize;
    for (member, by_name) in members {
        total += 1;
        if kept.len() >= HOLDS_MEMBERS {
            continue;
        }
        let all: usize = by_name.values().map(Vec::len).sum();
        let topics = by_name
            .into_iter()
            .take(HOLDS_TOPICS)
            .map(|(t, mut ps)| {
                ps.sort_unstable();
                ps.truncate(HOLDS_PARTITIONS);
                (clip(&t, 128).into(), ps)
            })
            .collect();
        kept.push(Hold {
            member,
            topics,
            total: count(all),
        });
    }
    (kept, count(total))
}

impl fmt::Display for Cause {
    /// The words the console says it in.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

impl Cause {
    /// A short title: the chip on a generation.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::MemberJoined => "Member joined",
            Self::MemberLeft => "Member left",
            Self::SessionExpired => "Session expired",
            Self::TopicChanged => "Topic changed",
            Self::ProtocolsChanged => "Rejoined with new protocols",
            Self::LeaderRejoined => "Leader rejoined",
            Self::RebalanceTimeout => "Rebalance timeout",
            Self::SubscriptionChanged => "Subscription changed",
            Self::RightsChanged => "Rights changed",
            Self::BecameClassic => "Became classic",
            Self::BecameConsumer => "Became a consumer group",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reason_is_kept_to_its_bytes_at_a_character_boundary() {
        let long = "é".repeat(REASON_BYTES);
        let kept = Reason::clip(&long).unwrap();
        assert_eq!(kept.as_str().len(), REASON_BYTES);
        let odd = format!("a{}", "é".repeat(REASON_BYTES));
        // 1 + 2n bytes: the last character that would cross the line is left out whole.
        assert_eq!(Reason::clip(&odd).unwrap().as_str().len(), REASON_BYTES - 1);
    }

    #[test]
    fn a_reason_with_nothing_in_it_is_no_reason_and_control_characters_are_left_out() {
        assert_eq!(Reason::clip(""), None);
        assert_eq!(Reason::clip("\u{1b}[31m\n\t"), Some(Reason("[31m".into())));
        assert_eq!(Reason::clip("\n\r"), None);
    }

    #[test]
    fn a_reason_of_900_kib_costs_no_more_than_a_short_one() {
        let huge = "x".repeat(900 * 1024);
        let kept = Reason::clip(&huge).unwrap();
        assert_eq!(kept.as_str().len(), REASON_BYTES);
        let note = Note {
            at: Millis(1),
            what: What::Dropped {
                generation: 1,
                who: Named::new(&MemberId::new("m").unwrap(), &huge, Some(kept)),
                wait_ms: 1,
            },
        };
        assert!(note.weight() < 1024, "{}", note.weight());
    }

    #[test]
    fn the_holds_of_a_big_group_are_clipped_and_counted() {
        let many = (0..40).map(|i| {
            (
                MemberId::new(&format!("m{i}")).unwrap(),
                std::collections::BTreeMap::from([("t".to_owned(), (0..50).collect::<Vec<i32>>())]),
            )
        });
        let (kept, total) = holds(many);
        assert_eq!((kept.len(), total), (HOLDS_MEMBERS, 40));
        assert_eq!(kept[0].topics[0].1.len(), HOLDS_PARTITIONS);
        assert_eq!(kept[0].total, 50, "what is held is counted whole");
    }
}
