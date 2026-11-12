//! What a group has said of itself: why it rebalanced, who it was about, what came of it. The ring the group's own task
//! keeps (see `groups::events`) read as owned, plain values.
//!
//! Nothing here is on disk: a restart empties every ring, and so does a group's deletion. Asking is a request to the
//! group's task like any other, which changes nothing.

use kafka_auth::authz::{operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ids::GroupId;

use super::ConsoleView;
use crate::Caller;
use crate::clock;
use crate::groups::Cause;
use crate::groups::events::{EVENTS_PER_GROUP, GROUPS_KEPT, RING_BYTES, Recorded};
use crate::groups::note::{Hold, Named, What};

/// What a note says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A classic group began a rebalance (a new generation is forming).
    Began,
    /// The wait ended and the generation was formed.
    Formed,
    /// A member did not join again in time and was dropped.
    Dropped,
    /// The leader's assignment was handed out: the generation is running.
    Settled,
    /// A consumer group's epoch moved.
    Epoch,
}

/// What one member holds in a generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberHold {
    pub member: String,
    /// Topic name and the partitions of it held (the first few).
    pub topics: Vec<(String, Vec<i32>)>,
    /// How many partitions in all.
    pub total: u32,
}

/// One note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupEvent {
    pub kind: EventKind,
    /// When it was said, milliseconds since the epoch.
    pub at_ms: i64,
    /// The generation (classic) or epoch (consumer group) it is about.
    pub generation: i32,
    /// Why, for the notes that begin a generation or move an epoch, and for a member dropped as one formed.
    pub cause: Option<Cause>,
    /// The member it is about, by id.
    pub member: Option<String>,
    /// The `client.id` that member joined with.
    pub client: Option<String>,
    /// How many more members did the same in the same request.
    pub more: u32,
    /// What the client said when it joined or left, clipped to 128 bytes.
    pub reason: Option<String>,
    /// The session timeout the member had, where the broker knew it (a member that expired: how long it was silent).
    pub session_ms: Option<u64>,
    /// How many members the group had before and after a rebalance began or an epoch moved.
    pub members_before: Option<u32>,
    pub members_after: Option<u32>,
    /// A rebalance that began: how long the members were given to join again. A member dropped: how long it was waited
    /// for.
    pub wait_ms: Option<u64>,
    /// A generation formed: its members (the first few), how many there are in all, and its leader.
    pub members: Vec<String>,
    pub members_total: u32,
    pub leader: Option<String>,
    /// A generation settled: what each member holds (the first few members).
    pub holds: Vec<MemberHold>,
    pub holds_total: u32,
}

/// What a group has said of itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupEvents {
    pub group: String,
    /// The moment of the look, milliseconds since the epoch.
    pub now_ms: i64,
    /// Oldest first.
    pub events: Vec<GroupEvent>,
    /// The most notes a group keeps, the most groups that keep any, and the most a group's notes weigh in bytes.
    pub events_per_group: usize,
    pub groups_kept: usize,
    pub ring_bytes: usize,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// What `group` has said of itself since the broker started, if `caller` may DESCRIBE it. Empty when the group has
    /// said nothing, and `None` when the group is not in memory (the broker was restarted, or it is deleted): there is
    /// nothing to look at, which is not the same as nothing having happened.
    pub async fn group_events(&self, caller: &Caller, group: &str) -> Option<GroupEvents> {
        let b = self.broker;
        if !b
            .authz
            .allows(caller, resource::GROUP, group, operation::DESCRIBE)
        {
            return None;
        }
        let id = GroupId::parse(group).ok()?;
        let recorded = b.groups.events(&id).await?;
        let now_ms = clock::now().get();
        Some(GroupEvents {
            group: group.to_owned(),
            now_ms,
            events: recorded.iter().map(|r| event_of(r, now_ms)).collect(),
            events_per_group: EVENTS_PER_GROUP,
            groups_kept: GROUPS_KEPT,
            ring_bytes: RING_BYTES,
        })
    }
}

fn event_of(r: &Recorded, now_ms: i64) -> GroupEvent {
    let ago = i64::try_from(r.ago_ms).unwrap_or(i64::MAX);
    let mut e = GroupEvent {
        kind: EventKind::Began,
        at_ms: now_ms.saturating_sub(ago),
        generation: 0,
        cause: None,
        member: None,
        client: None,
        more: 0,
        reason: None,
        session_ms: None,
        members_before: None,
        members_after: None,
        wait_ms: None,
        members: Vec::new(),
        members_total: 0,
        leader: None,
        holds: Vec::new(),
        holds_total: 0,
    };
    match &r.note.what {
        What::Began {
            why,
            generation,
            after,
            wait_ms,
        } => {
            e.kind = EventKind::Began;
            e.generation = *generation;
            e.cause = Some(why.cause);
            e.named(why.who.as_ref());
            e.more = why.more;
            e.members_before = Some(why.before);
            e.members_after = Some(*after);
            e.wait_ms = Some(*wait_ms);
        }
        What::Epoch { why, epoch, after } => {
            e.kind = EventKind::Epoch;
            e.generation = *epoch;
            e.cause = Some(why.cause);
            e.named(why.who.as_ref());
            e.more = why.more;
            e.members_before = Some(why.before);
            e.members_after = Some(*after);
        }
        What::Formed {
            generation,
            members,
            total,
            leader,
        } => {
            e.kind = EventKind::Formed;
            e.generation = *generation;
            e.members = members.iter().map(ToString::to_string).collect();
            e.members_total = *total;
            e.leader = leader.as_ref().map(ToString::to_string);
        }
        What::Dropped {
            generation,
            who,
            wait_ms,
        } => {
            e.kind = EventKind::Dropped;
            e.generation = *generation;
            e.cause = Some(Cause::RebalanceTimeout);
            e.named(Some(who));
            e.wait_ms = Some(*wait_ms);
        }
        What::Settled {
            generation,
            holds,
            total,
        } => {
            e.kind = EventKind::Settled;
            e.generation = *generation;
            e.holds = holds.iter().map(hold_of).collect();
            e.holds_total = *total;
        }
    }
    e
}

impl GroupEvent {
    fn named(&mut self, who: Option<&Named>) {
        let Some(w) = who else { return };
        self.member = Some(w.member.to_string());
        self.client = (!w.client.is_empty()).then(|| w.client.to_string());
        self.reason = w.reason.as_ref().map(|r| r.as_str().to_owned());
        self.session_ms = w.session_ms;
    }
}

fn hold_of(h: &Hold) -> MemberHold {
    MemberHold {
        member: h.member.to_string(),
        topics: h
            .topics
            .iter()
            .map(|(t, ps)| (t.to_string(), ps.clone()))
            .collect(),
        total: h.total,
    }
}
