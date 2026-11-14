//! The group boards' notes: what the broker's ring would say of the larder's groups, as `GroupEvents`, which the same
//! assembly the live pages use turns into the Story. `fulfilment` is the board's rebalancing group (four generations, the
//! newest still forming), `payments-workers` a KIP-848 group (three epochs), `search-indexer` the calm one (one generation);
//! `analytics` has said nothing since the broker started.
//!
//! The clock of the notes is the Story boards' (Nov 9, 22:14:22.3); a classic group cannot say "a topic changed", so the
//! generation the board draws with that cause is a leader that joined again, which is what the broker would have seen.

use kafka_broker::{Cause, EventKind, GroupEvent, GroupEvents, MemberHold};

use crate::format::parse_time;
use crate::model::StoryView;

mod world;

pub use world::{calm_world, flapping_world};

/// The look: 22:14:22.311 on the day of the world.
#[must_use]
pub fn look() -> i64 {
    parse_time("2026-11-09 22:14:22").unwrap_or(0) + 311
}

/// A time of the day of the world, `hh:mm:ss` and milliseconds.
pub(crate) fn at(clock: &str, ms: i64) -> i64 {
    parse_time(&format!("2026-11-09 {clock}")).unwrap_or(0) + ms
}

/// What a member held: the topics and the partitions of each.
pub(crate) type Parts<'a> = &'a [(&'a str, &'a [i32])];
/// What each member held.
pub(crate) type Held<'a> = &'a [(&'a str, Parts<'a>)];

/// A note with nothing in it but its kind, time and generation.
pub(crate) fn blank(kind: EventKind, at_ms: i64, generation: i32) -> GroupEvent {
    GroupEvent {
        kind,
        at_ms,
        generation,
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
    }
}

/// A rebalance that began: its cause, who it was about, the members before and after, the wait the members were given.
pub(crate) struct Began<'a> {
    pub(crate) at_ms: i64,
    pub(crate) generation: i32,
    pub(crate) cause: Cause,
    pub(crate) who: Option<&'a str>,
    pub(crate) session_ms: Option<u64>,
    pub(crate) members: (u32, u32),
    pub(crate) wait_ms: u64,
}

pub(crate) fn began(b: &Began<'_>) -> GroupEvent {
    let mut e = blank(EventKind::Began, b.at_ms, b.generation);
    e.cause = Some(b.cause);
    e.member = b.who.map(str::to_owned);
    e.client = b
        .who
        .map(|w| w.rsplit_once('-').map_or(w, |(c, _)| c).to_owned());
    e.session_ms = b.session_ms;
    e.members_before = Some(b.members.0);
    e.members_after = Some(b.members.1);
    e.wait_ms = Some(b.wait_ms);
    e
}

/// The generation formed: its members and its leader.
pub(crate) fn formed(at_ms: i64, generation: i32, members: &[&str]) -> GroupEvent {
    let mut e = blank(EventKind::Formed, at_ms, generation);
    e.members = members.iter().map(|m| (*m).to_owned()).collect();
    e.members_total = u32::try_from(members.len()).unwrap_or(0);
    e.leader = members.first().map(|m| (*m).to_owned());
    e
}

/// The assignment handed out: what each member holds, as (member, topic, partitions).
pub(crate) fn settled(at_ms: i64, generation: i32, held: Held<'_>) -> GroupEvent {
    let mut e = blank(EventKind::Settled, at_ms, generation);
    e.holds = held
        .iter()
        .map(|(member, topics)| MemberHold {
            member: (*member).to_owned(),
            topics: topics
                .iter()
                .map(|(t, ps)| ((*t).to_owned(), ps.to_vec()))
                .collect(),
            total: u32::try_from(topics.iter().map(|(_, ps)| ps.len()).sum::<usize>()).unwrap_or(0),
        })
        .collect();
    e.holds_total = u32::try_from(e.holds.len()).unwrap_or(0);
    e
}

/// What the ring of `id` holds in the world, if the broker has the group in memory.
#[must_use]
pub fn events(id: &str) -> Option<GroupEvents> {
    let events = match id {
        "fulfilment" => world::fulfilment(),
        "payments-workers" => world::payments_workers(),
        "search-indexer" => world::search_indexer(),
        _ => return None,
    };
    Some(GroupEvents {
        group: id.to_owned(),
        now_ms: look(),
        events,
        events_per_group: 200,
        groups_kept: 100,
        ring_bytes: 65_536,
    })
}

/// The Story of a group of the world; `None` for a name that is no group of it.
#[must_use]
pub fn story(id: &str) -> Option<StoryView> {
    let lag = crate::fixtures_groups::group(id, false)?;
    let others: Vec<(String, GroupEvents)> = lag
        .head
        .groups
        .iter()
        .filter(|g| g.as_str() != id)
        .filter_map(|g| events(g).map(|e| (g.clone(), e)))
        .collect();
    Some(crate::gather::story::assemble(lag, events(id), &others))
}
