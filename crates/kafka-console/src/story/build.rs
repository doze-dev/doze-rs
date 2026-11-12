//! The ring of notes made into generations: each rebalance that began, with what came of it, newest first.

use kafka_broker::{Cause, EventKind, GroupEvent, GroupEvents};

use super::flap::flap_of;
use crate::model::{
    GroupHead, GroupPhase, GroupProtocol, StoryCalm, StoryCause, StoryDropped, StoryFacts,
    StoryGen, StoryHold, StoryJoin, StoryLast, StoryLeft, StoryNow, StoryTone, StoryView, StoryWho,
};

const fn cause_of(c: Cause) -> StoryCause {
    match c {
        Cause::MemberJoined => StoryCause::MemberJoined,
        Cause::MemberLeft => StoryCause::MemberLeft,
        Cause::SessionExpired => StoryCause::SessionExpired,
        Cause::TopicChanged => StoryCause::TopicChanged,
        Cause::ProtocolsChanged => StoryCause::ProtocolsChanged,
        Cause::LeaderRejoined => StoryCause::LeaderRejoined,
        Cause::RebalanceTimeout => StoryCause::RebalanceTimeout,
        Cause::SubscriptionChanged => StoryCause::SubscriptionChanged,
        Cause::RightsChanged => StoryCause::RightsChanged,
        Cause::BecameClassic => StoryCause::BecameClassic,
        Cause::BecameConsumer => StoryCause::BecameConsumer,
    }
}

fn who_of(e: &GroupEvent) -> Option<StoryWho> {
    e.member.as_ref().map(|member| StoryWho {
        member: member.clone(),
        client: e.client.clone(),
        reason: e.reason.clone(),
        session_ms: e.session_ms,
    })
}

const fn tone_of(cause: StoryCause, forming: bool) -> StoryTone {
    match cause {
        _ if forming => StoryTone::Warn,
        StoryCause::SessionExpired | StoryCause::RebalanceTimeout => StoryTone::Bad,
        StoryCause::TopicChanged => StoryTone::Idle,
        _ => StoryTone::Plain,
    }
}

fn gap(later: i64, earlier: i64) -> u64 {
    u64::try_from(later.saturating_sub(earlier)).unwrap_or(0)
}

/// The generation a note begins.
fn begun(e: &GroupEvent, protocol: GroupProtocol) -> Option<StoryGen> {
    let cause = cause_of(e.cause?);
    let forming = e.kind == EventKind::Began;
    Some(StoryGen {
        generation: e.generation,
        protocol,
        cause,
        at_ms: e.at_ms,
        who: who_of(e),
        more: e.more,
        before: e.members_before.unwrap_or(0),
        after: e.members_after.unwrap_or(0),
        wait_ms: e.wait_ms,
        formed_ms: None,
        settled_ms: None,
        forming,
        members: Vec::new(),
        members_total: e.members_after.unwrap_or(0),
        leader: None,
        holds: Vec::new(),
        holds_total: 0,
        dropped: Vec::new(),
        lasted_ms: None,
        moves: Vec::new(),
        tone: tone_of(cause, forming),
    })
}

/// What a later note adds to the generation it is about.
fn add(g: &mut StoryGen, e: &GroupEvent) {
    match e.kind {
        EventKind::Formed => {
            g.forming = false;
            g.formed_ms = Some(gap(e.at_ms, g.at_ms));
            g.members.clone_from(&e.members);
            g.members_total = e.members_total;
            g.after = e.members_total;
            g.leader.clone_from(&e.leader);
            g.tone = tone_of(g.cause, false);
        }
        EventKind::Settled => {
            g.settled_ms = Some(gap(e.at_ms, g.at_ms));
            g.holds = e
                .holds
                .iter()
                .map(|h| StoryHold {
                    member: h.member.clone(),
                    topics: h.topics.clone(),
                    total: h.total,
                })
                .collect();
            g.holds_total = e.holds_total;
        }
        EventKind::Dropped => {
            if let Some(who) = who_of(e) {
                g.dropped.push(StoryDropped {
                    who,
                    waited_ms: e.wait_ms.unwrap_or(0),
                });
            }
        }
        EventKind::Began | EventKind::Epoch => {}
    }
}

/// Every rebalance the ring holds, as generations, newest first. A note about a generation whose beginning was let go
/// (the ring is bounded) is not drawn: there is nothing to hang it on.
#[must_use]
pub fn generations_of(events: &GroupEvents, protocol: GroupProtocol, now_ms: i64) -> Vec<StoryGen> {
    let mut gens: Vec<StoryGen> = Vec::new();
    for e in &events.events {
        match e.kind {
            EventKind::Began | EventKind::Epoch => gens.extend(begun(e, protocol)),
            _ => {
                if let Some(g) = gens.iter_mut().rev().find(|g| g.generation == e.generation) {
                    add(g, e);
                }
            }
        }
    }
    let starts: Vec<i64> = gens.iter().map(|g| g.at_ms).collect();
    for (i, g) in gens.iter_mut().enumerate() {
        g.lasted_ms = Some(gap(starts.get(i + 1).copied().unwrap_or(now_ms), g.at_ms));
    }
    attach_moves(&mut gens);
    gens.reverse();
    gens
}

/// For a member that left, went silent or was dropped: what it held in the generation before, which has to move.
fn attach_moves(gens: &mut [StoryGen]) {
    for i in 1..gens.len() {
        let (older, rest) = gens.split_at_mut(i);
        let Some(g) = rest.first_mut() else { continue };
        let mut gone: Vec<String> = g.dropped.iter().map(|d| d.who.member.clone()).collect();
        if matches!(
            g.cause,
            StoryCause::MemberLeft | StoryCause::SessionExpired | StoryCause::RebalanceTimeout
        ) {
            gone.extend(g.who.iter().map(|w| w.member.clone()));
        }
        let Some(before) = older.iter().rev().find(|g| !g.holds.is_empty()) else {
            continue;
        };
        g.moves = before
            .holds
            .iter()
            .filter(|h| gone.contains(&h.member) && !h.topics.is_empty())
            .map(|h| (h.member.clone(), h.topics.clone()))
            .collect();
    }
}

/// How many rebalances the ring holds and the newest, for a group's tabs and sentence.
#[must_use]
pub fn facts_of(events: Option<&GroupEvents>, protocol: GroupProtocol) -> StoryFacts {
    let Some(events) = events else {
        return StoryFacts::default();
    };
    let gens = generations_of(events, protocol, events.now_ms);
    let joins: Vec<StoryJoin> = gens
        .iter()
        .filter(|g| g.cause == StoryCause::MemberJoined)
        .filter_map(|g| {
            g.who.as_ref().map(|w| StoryJoin {
                member: w.member.clone(),
                ago_ms: gap(events.now_ms, g.at_ms),
                generation: g.generation,
            })
        })
        .collect();
    let left = gens
        .first()
        .filter(|g| {
            g.forming && matches!(g.cause, StoryCause::MemberLeft | StoryCause::SessionExpired)
        })
        .and_then(|g| {
            let w = g.who.as_ref()?;
            Some(StoryLeft {
                member: w.member.clone(),
                held: g
                    .moves
                    .iter()
                    .find(|(m, _)| *m == w.member)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default(),
                ago_ms: gap(events.now_ms, g.at_ms),
                cause: g.cause,
            })
        });
    StoryFacts {
        count: gens.len(),
        joins,
        left,
        last: gens.first().map(|g| StoryLast {
            cause: g.cause,
            generation: g.generation,
            who: g.who.as_ref().map(|w| w.member.clone()),
            ago_ms: gap(events.now_ms, g.at_ms),
            forming: g.forming,
        }),
    }
}

/// How many of each cause, most frequent first, then by the order the page lists them.
#[must_use]
pub fn causes_of(gens: &[StoryGen]) -> Vec<(StoryCause, usize)> {
    let mut out: Vec<(StoryCause, usize)> = StoryCause::ALL
        .iter()
        .map(|c| (*c, gens.iter().filter(|g| g.cause == *c).count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    out
}

/// What the group is doing now, from its head (which the broker described a moment ago) and its newest generation.
#[must_use]
pub fn now_of(head: &GroupHead, gens: &[StoryGen], now_ms: i64) -> StoryNow {
    let newest = gens.first();
    let rebalance = head.rebalance.as_ref();
    let forming = rebalance.is_some() || newest.is_some_and(|g| g.forming);
    StoryNow {
        phase: head.phase,
        protocol: head.protocol,
        generation: if head.protocol == GroupProtocol::Classic && forming {
            head.generation.saturating_add(1)
        } else {
            head.generation
        },
        began_ms: newest.map(|g| g.at_ms),
        cause: newest.map(|g| g.cause),
        who: newest.and_then(|g| g.who.clone()),
        waiting_for: rebalance.map(|r| r.waiting_for.clone()).unwrap_or_default(),
        remaining_ms: rebalance.map(|r| r.remaining_ms),
        timeout_ms: rebalance.map(|r| r.timeout_ms),
        held_ms: newest
            .filter(|_| head.phase != GroupPhase::Empty)
            .map(|g| gap(now_ms, g.at_ms)),
    }
}

/// Another group that rebalances rarely, to set beside this one: its only (or newest) generation.
#[must_use]
pub fn calm_of(group: &str, gens: &[StoryGen]) -> Option<StoryCalm> {
    let newest = gens.first()?;
    (gens.len() <= 2 && !newest.forming).then(|| StoryCalm {
        group: group.to_owned(),
        generation: newest.clone(),
        count: gens.len(),
    })
}

/// The Story page of a group.
#[must_use]
pub fn view_of(
    head: GroupHead,
    events: Option<&GroupEvents>,
    calm: Option<StoryCalm>,
    now_ms: i64,
) -> StoryView {
    let protocol = head.protocol;
    let gens = events.map_or_else(Vec::new, |e| generations_of(e, protocol, now_ms));
    let oldest_first: Vec<StoryGen> = gens.iter().rev().cloned().collect();
    StoryView {
        now: now_of(&head, &gens, now_ms),
        causes: causes_of(&gens),
        flap: flap_of(&oldest_first, now_ms),
        since_ms: events.and_then(|e| e.events.first().map(|n| n.at_ms)),
        full: events.is_some_and(|e| e.events.len() >= e.events_per_group),
        events_per_group: events.map_or(200, |e| e.events_per_group),
        in_memory: events.is_some(),
        generations: gens,
        calm,
        head,
    }
}

/// A story with one rebalance in it, settled, in a group that is Stable: nothing to read but that one line.
#[must_use]
pub fn is_quiet(v: &StoryView) -> bool {
    v.in_memory
        && v.head.phase == GroupPhase::Stable
        && matches!(v.generations.as_slice(), [g] if !g.forming)
}
