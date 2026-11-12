//! `group-story`: the Story tab of a group, and the page for a group the console cannot show.

use maud::{Markup, html};

use super::cards::{calm, causes, flapping_card, now, quiet};
use super::generation::generation;
use crate::base::Base;
use crate::model::{GroupHead, StoryView};
use crate::pages::groups::{group_head, group_story_tabs, group_switch, group_url};
use crate::story::{cause_phrase, is_quiet, lasted};
use crate::story_ids::{STORY_NOW, STORY_SENTENCE};
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};

/// `/groups/{group}/story` and what follows it.
#[must_use]
pub fn story_url(base: &Base, id: &str, tail: &str) -> String {
    group_url(base, id, &format!("/story{tail}"))
}

/// The sentence under the group's name: how many times it rebalanced, why, and what the newest is doing.
fn sentence(v: &StoryView) -> Markup {
    let n = v.generations.len();
    let id = &v.head.id;
    html! {
        @if is_quiet(v) {
            (quiet_sentence(v))
        } @else if !v.in_memory {
            span class="mono" { (id) } " has nothing in the broker's memory: it was started after the group last had members, so what happened before is not kept."
        } @else if n == 0 {
            span class="mono" { (id) } " has not rebalanced since the broker started."
        } @else {
            span class="mono" { (id) } " has rebalanced " (n) @if n == 1 { " time" } @else { " times" }
            @if let Some(since) = v.since_ms { " in the last " (lasted(u64::try_from(v.head.now_ms.max(since) - since).unwrap_or(0).max(60_000))) }
            @if n == 1 {
                ": " (v.generations.first().map_or("", |g| cause_phrase(g.cause)))
            } @else if v.causes.len() == n {
                ", each for a different reason: " (reasons(v))
            } @else {
                ": " (reasons(v))
            }
            "."
            @if v.generations.first().is_some_and(|g| g.forming) { " The newest is still going: " (still_going(v)) }
        }
    }
}

fn reasons(v: &StoryView) -> String {
    let each: Vec<String> = if v.causes.len() == v.generations.len() {
        // Oldest first, as they happened.
        v.generations
            .iter()
            .rev()
            .map(|g| cause_phrase(g.cause).to_owned())
            .collect()
    } else {
        v.causes
            .iter()
            .map(|(c, n)| format!("{} ({n})", cause_phrase(*c)))
            .collect()
    };
    crate::words::list_and(&each)
}

fn footnote(v: &StoryView) -> Markup {
    let oldest = v.generations.last().map_or(0, |g| g.generation);
    html! {
        p class="help" style="margin-top:4px;max-width:none" {
            @if v.full && oldest > 1 {
                "Generations 1 to " (oldest - 1) " are older than the ring keeps: "
            } @else {
                "This is everything since the broker started: the ring keeps "
            }
            (v.events_per_group) " events per group, emptied at a restart."
        }
    }
}

/// The page of a group with a quiet story: one rebalance, and nothing since. No Now card and no cards of causes: a line, and
/// the reason a quiet story is a good one.
fn quiet_page(v: &StoryView, base: &Base) -> Markup {
    let h: &GroupHead = &v.head;
    let lag = html! { a class="btn" href=(group_url(base, &h.id, "/lag")) { (group_glyph(GroupGlyph::Back, 16, "")) "See its lag" } };
    html! {
        (group_head(h, sentence_fragment(v, base), lag))
        (group_switch(h, base, "/story"))
        (group_story_tabs(h, base))
        div class="split wide" {
            div {
                div class="story" data-extra="story" { @for g in &v.generations { (generation(g, h, base, true, true)) } }
                (footnote(v))
            }
            (quiet(v, base))
        }
    }
}

/// The page.
#[must_use]
pub fn story_page(v: &StoryView, base: &Base) -> Markup {
    if is_quiet(v) {
        return quiet_page(v, base);
    }
    let h: &GroupHead = &v.head;
    let wire = link(
        &group_url(base, &h.id, "/wire"),
        Variant::Plain,
        Size::Normal,
        Some(Glyph::List),
        "Group wire",
    );
    html! {
        (group_head(h, sentence_fragment(v, base), wire))
        (group_switch(h, base, "/story"))
        (group_story_tabs(h, base))
        div class="split wide" {
            div {
                (now_fragment(v, base))
                h2 style="margin-bottom:20px" { "Generations, newest first" }
                @if v.generations.is_empty() {
                    p class="help" style="max-width:none" { "Nothing to tell yet. The first member to join, leave or time out is the first line of the story." }
                } @else {
                    div class="story" data-extra="story" {
                        @for (i, g) in v.generations.iter().enumerate() { (generation(g, h, base, i == 0, true)) }
                    }
                }
                (footnote(v))
            }
            div class="stack-y" {
                (causes(v))
                (flapping_card(v, base))
                (calm(v, base))
            }
        }
    }
}

/// A group that is not there.
#[must_use]
pub fn no_story(id: &str, base: &Base) -> Markup {
    crate::pages::groups::no_group(id, base)
}

/// What the newest rebalance, still going, waits for.
fn still_going(v: &StoryView) -> Markup {
    let n = &v.now;
    html! {
        @if n.waiting_for.is_empty() { "every member has joined again, and the leader is working out who holds what." }
        @else {
            @for (i, m) in n.waiting_for.iter().enumerate() { @if i > 0 { ", " } span class="mono" { (crate::story::member_label(m)) } }
            @if let (Some(left), Some(of)) = (n.remaining_ms, n.timeout_ms) {
                " has " (crate::groups_words::span_precise(left)) " of " (crate::groups_words::span_precise(of)) " left to rejoin."
            } @else { " has not rejoined." }
        }
    }
}

fn fragment_url(base: &Base, id: &str, group: &str) -> String {
    base.url(&format!(
        "/fragments/{id}?group={}",
        crate::base::segment(group)
    ))
}

/// The sentence under the name, in its fragment: it follows the group while the page is open.
#[must_use]
pub fn sentence_fragment(v: &StoryView, base: &Base) -> Markup {
    client::fragment(
        STORY_SENTENCE,
        &fragment_url(base, STORY_SENTENCE, &v.head.id),
        html! { span data-extra="live-sentence" data-vary { (sentence(v)) } },
    )
}

/// The Now card, in its fragment: the time the members have left counts down as the page refreshes.
#[must_use]
pub fn now_fragment(v: &StoryView, base: &Base) -> Markup {
    client::fragment(
        STORY_NOW,
        &fragment_url(base, STORY_NOW, &v.head.id),
        now(v, base),
    )
}

/// "One rebalance in the last 6 h: search-indexer-1 joined at 18:30:12, 3 h 44 m ago. Nothing has moved since."
fn quiet_sentence(v: &StoryView) -> Markup {
    let Some(g) = v.generations.first() else {
        return html! {};
    };
    let ago = lasted(g.lasted_ms.unwrap_or(0));
    html! {
        "One rebalance in the last " (ago) ": "
        @match &g.who {
            Some(w) => { span class="bright" title=(w.member) { (crate::story::member_label(&w.member)) } " " (verb(g.cause)) },
            None => (cause_phrase(g.cause)),
        }
        " at " (crate::format::clock(g.at_ms)) ", " (ago) " ago. Nothing has moved since."
    }
}

fn verb(c: crate::model::StoryCause) -> &'static str {
    use crate::model::StoryCause::{MemberJoined, MemberLeft, SessionExpired};
    match c {
        MemberLeft => "left",
        SessionExpired => "timed out",
        MemberJoined => "joined",
        _ => "was the cause",
    }
}
