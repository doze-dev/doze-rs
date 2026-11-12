//! One generation of the timeline, as the boards draw it: the time, a dot, the cause, what happened, what came of it.

use maud::{Markup, html};

use super::story_url;
use crate::base::Base;
use crate::format::clock;
use crate::model::{GroupHead, GroupProtocol, StoryCause, StoryGen, StoryTone, StoryWho};
use crate::story::{cause_title, holds_phrase, lasted, member_label};

/// A member in a sentence: its name, bold, with the whole id on hover.
#[must_use]
pub(super) fn who(w: &StoryWho) -> Markup {
    html! { b class="bright" title=(w.member) { (member_label(&w.member)) } }
}

fn some_who(w: Option<&StoryWho>) -> Markup {
    w.map_or_else(|| html! { "a member" }, who)
}

fn tone(t: StoryTone) -> &'static str {
    match t {
        StoryTone::Warn => "ev warn",
        StoryTone::Bad => "ev bad",
        StoryTone::Idle => "ev idle",
        StoryTone::Plain => "ev",
    }
}

/// `Generation 31 is forming`, `Epoch 14`.
#[must_use]
pub(super) fn title(g: &StoryGen) -> String {
    let word = match g.protocol {
        GroupProtocol::Classic => "Generation",
        GroupProtocol::Consumer => "Epoch",
    };
    if g.forming {
        format!("{word} {} is forming", g.generation)
    } else {
        format!("{word} {}", g.generation)
    }
}

fn members(g: &StoryGen) -> String {
    format!("Members {} to {}.", g.before, g.after)
}

/// What the generation was about, in a sentence.
#[must_use]
pub(super) fn what(g: &StoryGen) -> Markup {
    let w = g.who.as_ref();
    let consumer = g.protocol == GroupProtocol::Consumer;
    html! {
        @match g.cause {
            StoryCause::MemberJoined => {
                (some_who(w)) @if g.before == 0 { " joined an empty group and led it. " } @else { " joined. " } (members(g))
            },
            StoryCause::MemberLeft => {
                (some_who(w)) @if consumer { " left the group. " } @else { " sent LeaveGroup. " } (members(g))
                @if let Some(r) = w.and_then(|w| w.reason.as_ref()) { " It said: " q { (r) } "." }
            },
            StoryCause::SessionExpired => {
                (some_who(w))
                @match w.and_then(|w| w.session_ms) {
                    Some(ms) => { " sent no heartbeat for " (crate::groups_words::seconds(ms)) ", its session timeout. " },
                    None => { " sent no heartbeat within its session timeout. " },
                }
                (members(g))
            },
            StoryCause::TopicChanged => {
                "A topic this group reads changed: partitions were added, or a topic was made or deleted. The broker moved the group on and worked out who holds what again."
            },
            StoryCause::ProtocolsChanged => { (some_who(w)) " joined again offering other protocols than before. " (members(g)) },
            StoryCause::LeaderRejoined => {
                "The leader, " (some_who(w)) ", joined again with what it had. Kafka rebalances when the leader does; clients do it when a topic they read changes."
            },
            StoryCause::RebalanceTimeout => { (some_who(w)) " did not give up what it was told to within its rebalance timeout, and was removed. " (members(g)) },
            StoryCause::SubscriptionChanged => { (some_who(w)) " asked for other topics, a pattern or an assignor. " (members(g)) },
            StoryCause::RightsChanged => { "A right to read a topic was given to, or taken from, the principal of " (some_who(w)) "." },
            StoryCause::BecameClassic => { "The last consumer-protocol member left, so the group is a classic group again and every member joins again." },
            StoryCause::BecameConsumer => { "A member of the new protocol joined a classic group of consumers: it is a consumer group now, its members staying." },
        }
        @if let Some(ms) = g.settled_ms.or(g.formed_ms) { " Settled in " (crate::groups_words::span_precise(ms)) "." }
    }
}

/// The second line: what each member held, who did not come back, how long it lasted.
fn then(g: &StoryGen, newest: bool) -> Markup {
    // The generation running now says what its members hold; an older one, what they held.
    let present = newest && !g.forming;
    let alone = g.holds.len() == 1;
    let held: Vec<String> = g
        .holds
        .iter()
        .map(|h| holds_phrase(&h.member, &h.topics, present, alone))
        .collect();
    html! {
        @for d in &g.dropped {
            (who(&d.who)) " did not join again in " (crate::groups_words::span_precise(d.waited_ms)) " and was dropped. "
        }
        @if !held.is_empty() { (held.join("; ")) ". " }
        @if let Some(ms) = g.lasted_ms {
            @if newest { "Lasted " (lasted(ms)) " so far." } @else { "Lasted " (lasted(ms)) "." }
        }
    }
}

/// The members of the generation, as pills; the one that left is marked.
fn in_the_group(g: &StoryGen, gone: Option<&str>) -> Markup {
    html! {
        @if !g.members.is_empty() || gone.is_some() {
            div class="row wrap" style="margin-top:10px;gap:8px" {
                span class="dim" { "In the group:" }
                @for m in &g.members { span class="pill mono" title=(m) { (member_label(m)) } }
                @if g.members_total as usize > g.members.len() { span class="pill" { "and " (g.members_total as usize - g.members.len()) " more" } }
                @if let Some(left) = gone { span class="pill mono" title=(left) { (member_label(left)) " left" } }
            }
        }
    }
}

/// One generation of the timeline. `now_members` are the members the group has now, for a generation still forming;
/// `link` puts an "Open this rebalance" under it.
#[must_use]
pub(super) fn generation(
    g: &StoryGen,
    head: &GroupHead,
    base: &Base,
    newest: bool,
    link: bool,
) -> Markup {
    let mut shown = g.clone();
    if g.forming && shown.members.is_empty() {
        shown.members = head.members.iter().map(|m| m.id.clone()).collect();
        shown.members_total = u32::try_from(shown.members.len()).unwrap_or(0);
    }
    let gone = (g.forming && g.cause == StoryCause::MemberLeft)
        .then(|| g.who.as_ref().map(|w| w.member.as_str()))
        .flatten();
    html! {
        div class=(tone(g.tone)) {
            div class="tm" data-vary { (clock(g.at_ms)) }
            div class="dot" { i {} }
            div class="tx" {
                div { span class="cause" { (cause_title(g.cause)) } b { (title(g)) } }
                div class="dim" style="margin-top:6px" { (what(g)) }
                @if g.forming && newest {
                    @if !head.rebalance.as_ref().is_none_or(|r| r.waiting_for.is_empty()) {
                        (waiting(head))
                    }
                } @else {
                    div class="dim" style="margin-top:6px" { (then(g, newest)) }
                }
                (in_the_group(&shown, gone))
                @if link {
                    div style="margin-top:10px" {
                        a class="lnk" href=(story_url(base, &head.id, &format!("/{}", g.generation))) data-dialog { "Open this rebalance" }
                    }
                }
            }
        }
    }
}

/// `Not rejoined: py-1. 2.1 s left of 5 s.`
#[must_use]
pub(super) fn waiting(head: &GroupHead) -> Markup {
    let Some(r) = &head.rebalance else {
        return html! {};
    };
    html! {
        div class="dim" style="margin-top:6px" data-vary {
            "Not rejoined: "
            @for (i, m) in r.waiting_for.iter().enumerate() { @if i > 0 { ", " } b class="bright" title=(m) { (member_label(m)) } }
            ". " (crate::groups_words::span_precise(r.remaining_ms)) " left of " (crate::groups_words::span_precise(r.timeout_ms)) "."
        }
    }
}
