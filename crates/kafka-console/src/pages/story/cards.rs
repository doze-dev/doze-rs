//! The cards beside and above the timeline: Now, where the cause comes from, is it flapping, and a calm group for contrast.

use maud::{Markup, html};

use super::generation::{generation, who};
use super::story_url;
use crate::base::{Base, segment};
use crate::format::clock;
use crate::groups_words::span_precise;
use crate::model::{GroupPhase, GroupProtocol, StoryCause, StoryNow, StoryView};
use crate::story::{cause_help, cause_title, lasted};
use crate::ui::button::{Size, State, Variant, link, state};
use crate::ui::glyph::Glyph;

/// `22:14:19.4`: the clock to a tenth of a second, as the board writes the moment a generation began.
fn tenths(ms: i64) -> String {
    format!("{}.{}", clock(ms), ms.rem_euclid(1000) / 100)
}

fn tone(p: GroupPhase) -> State {
    match p {
        GroupPhase::Stable => State::Ok,
        GroupPhase::Empty => State::Idle,
        _ => State::Warn,
    }
}

/// What the group is doing, in a sentence.
fn now_sentence(n: &StoryNow, v: &StoryView) -> Markup {
    let word = if n.protocol == GroupProtocol::Classic {
        "Generation"
    } else {
        "Epoch"
    };
    html! {
        @match (n.phase, n.began_ms) {
            (GroupPhase::Rebalancing, Some(at)) => {
                (word) " " (n.generation) " began at " b class="mono" { (tenths(at)) }
                @if let Some(w) = &n.who { @match n.cause {
                    Some(StoryCause::MemberLeft) => { " when " (who(w)) " left" },
                    Some(StoryCause::SessionExpired) => { " when " (who(w)) " timed out" },
                    Some(StoryCause::MemberJoined) => { " when " (who(w)) " joined" },
                    _ => { " (" (n.cause.map_or("", crate::story::cause_phrase)) ")" },
                } }
                ". "
                @if n.waiting_for.is_empty() { "Every member has joined again; the leader is working out who holds what." }
                @else { "It is waiting for " @for (i, m) in n.waiting_for.iter().enumerate() { @if i > 0 { ", " } b class="bright mono" { (m) } } "." }
            },
            (GroupPhase::Empty, _) => { "Nobody is in the group." @if let Some(g) = v.generations.first() { " The last rebalance, " (word.to_lowercase()) " " (g.generation) ", ended with it empty." } },
            (_, Some(at)) => {
                (word) " " (n.generation) " began at " b class="mono" { (tenths(at)) }
                @if let Some(w) = &n.who { " when " (who(w)) @match n.cause { Some(StoryCause::MemberLeft) => " left", Some(StoryCause::SessionExpired) => " timed out", _ => " joined" } }
                ". It has run for " (n.held_ms.map_or_else(String::new, lasted)) "."
            },
            _ => { "The broker has not seen this group rebalance since it started." },
        }
    }
}

/// The time the members have left to join again, as a bar.
fn clock_bar(n: &StoryNow) -> Markup {
    let (Some(left), Some(of)) = (n.remaining_ms, n.timeout_ms) else {
        return html! {};
    };
    let pct = if of == 0 {
        0
    } else {
        (left.min(of) * 100 / of).min(100)
    };
    html! {
        div class="lagbar" data-vary {
            div class="tr" { i class="warn" style={"--p:" (pct) "%"} {} }
            span class="n" style="min-width:140px" { (span_precise(left)) " of " (span_precise(of)) " left" }
        }
    }
}

/// `Now`: the rebalance going on, and the time left for the member that has not joined again.
#[must_use]
pub(super) fn now(v: &StoryView, _base: &Base) -> Markup {
    let n = &v.now;
    html! {
        div class="card" style="margin-bottom:32px" data-extra="story" {
            div class="row sb" style="margin-bottom:6px" { h2 style="margin:0" { "Now" } (state(tone(n.phase), n.phase.word())) }
            p class="sent" style="margin:0 0 16px" { (now_sentence(n, v)) }
            @if n.phase == GroupPhase::Rebalancing { (clock_bar(n)) }
        }
    }
}

/// `Where the cause comes from`: what the broker records and the causes it can name, with how often each happened here.
#[must_use]
pub(super) fn causes(v: &StoryView) -> Markup {
    let seen = |c: StoryCause| {
        v.causes
            .iter()
            .find(|(k, _)| *k == c)
            .map_or(0, |(_, n)| *n)
    };
    let classic = v.head.protocol == GroupProtocol::Classic;
    html! {
        div class="card" data-extra="new" {
            div class="row sb" style="margin-bottom:8px" { h2 style="margin:0" { "Where the cause comes from" } span class="new" { "New" } }
            p class="help" style="max-width:none" {
                "Kafka's protocol has no call that says why a group rebalanced; a client sees only that it must rejoin. The broker knows, because it is the one deciding. It records the cause at that moment, with the member and how long the rebalance took."
                @if classic { " A topic that changed reaches a classic group as its leader joining again, which is how the cause is told here." }
            }
            div class="hr" style="margin:16px 0" {}
            div class="eyebrow" style="margin-bottom:10px" { "The causes it can name" }
            div class="stack-y" style="gap:12px" {
                @for c in StoryCause::ALL {
                    @if listed(classic, c) || seen(c) > 0 {
                        div { span class="cause" { (cause_title(c)) } span class="help" { (cause_help(c)) @if seen(c) > 0 { " " b class="mono" { "· " (seen(c)) " here" } } } }
                    }
                }
            }
        }
    }
}

/// `Is it flapping?`: the rule's verdict in a sentence, and a button to the rule held against the group.
#[must_use]
pub(super) fn flapping_card(v: &StoryView, base: &Base) -> Markup {
    html! {
        div class="card" {
            h2 { "Is it flapping?" }
            p class="help" style="max-width:none" { (super::flapping::verdict(v)) }
            div style="margin-top:12px" {
                (link(&story_url(base, &v.head.id, "/flapping"), Variant::Plain, Size::Normal, Some(Glyph::Search), "Check the rule"))
            }
        }
    }
}

/// `What a calm group looks like`: another group that has rebalanced once or twice, for contrast.
#[must_use]
pub(super) fn calm(v: &StoryView, base: &Base) -> Markup {
    let Some(c) = &v.calm else {
        return html! {
            div class="card" {
                h2 { "What a calm group looks like" }
                p class="help" style="max-width:none" { "A quiet story is a good one: one rebalance when the first member joined, and nothing since. If a group's lag grows and its story is quiet, the cause is not rebalancing: look at its Lag, and at how many partitions one process holds." }
            }
        };
    };
    let g = &c.generation;
    html! {
        div class="card" data-extra="story" {
            div class="row sb" style="margin-bottom:8px" { h2 style="margin:0" { "What a calm group looks like" } (state(State::Ok, "Stable")) }
            p class="help" style="max-width:none" {
                b class="mono" { a class="lnk" href=(base.url(&format!("/groups/{}/story", segment(&c.group)))) { (c.group) } }
                ", for contrast: " (c.count) @if c.count == 1 { " rebalance" } @else { " rebalances" } " since the broker started, and nothing has moved since " (clock(g.at_ms)) ". Its Story tab holds " @if c.count == 1 { "only this event." } @else { "these events." }
            }
            div class="story" style="margin-top:14px" { (generation(g, &v.head, base, false, false)) }
            div class="hr" style="margin:16px 0" {}
            p class="help" style="max-width:none" {
                "A quiet story is a good one. No member has come, gone or timed out since " (clock(g.at_ms)) ", so the lag on that group is not caused by rebalancing. Look at "
                a class="lnk" href=(base.url(&format!("/groups/{}/lag", segment(&c.group)))) { "its Lag" } " and at how many partitions one process holds."
            }
        }
    }
}

/// The causes a group of this protocol can have, which the card lists whether or not they have happened. The others
/// are listed when they have.
const fn listed(classic: bool, c: StoryCause) -> bool {
    if classic {
        matches!(
            c,
            StoryCause::MemberJoined
                | StoryCause::MemberLeft
                | StoryCause::SessionExpired
                | StoryCause::ProtocolsChanged
                | StoryCause::LeaderRejoined
                | StoryCause::RebalanceTimeout
        )
    } else {
        matches!(
            c,
            StoryCause::MemberJoined
                | StoryCause::MemberLeft
                | StoryCause::SessionExpired
                | StoryCause::TopicChanged
                | StoryCause::SubscriptionChanged
                | StoryCause::RebalanceTimeout
        )
    }
}
