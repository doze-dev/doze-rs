//! `story-flapping`: the rule held against the group, and what it looks like when a group is flapping.

use maud::{Markup, html};

use super::story_url;
use crate::base::Base;
use crate::model::{StoryFlap, StoryView};
use crate::pages::groups::{group_head, group_story_tabs};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};

/// The verdict in a sentence, for the card on the story and the head of this page.
#[must_use]
pub(super) fn verdict(v: &StoryView) -> String {
    let f = &v.flap;
    if f.flapping {
        format!(
            "Yes, by the rule: {} expired-then-joined cycles in the last 10 minutes. A member keeps timing out and coming back.",
            f.cycles
        )
    } else {
        let expired = match f.expiries {
            0 => "no session has expired".to_owned(),
            1 => "one session expiry".to_owned(),
            n => format!("{n} session expiries"),
        };
        format!(
            "Not by the rule: {expired} since the broker started, and a clean leave never counts.{}",
            watch(f)
        )
    }
}

fn watch(f: &StoryFlap) -> String {
    f.rows
        .iter()
        .find(|r| !r.ok && r.title.starts_with("A member joined and left"))
        .map_or_else(String::new, |r| {
            format!(
                " A member did join and leave within {}; worth a look if it repeats.",
                r.value
            )
        })
}

fn rules(f: &StoryFlap) -> Markup {
    html! {
        div class="rules" data-extra="teaching" {
            @for r in &f.rows {
                div class={"r " @if r.ok { "ok" } @else { "no" }} {
                    span class="gl" { (glyph(if r.ok { Glyph::Check } else { Glyph::Warn }, 16, "")) }
                    div { (r.title) span class="sub" { (r.detail) } }
                    span class="v" { (r.value) }
                }
            }
        }
    }
}

/// The picture of a group that is: a client that pauses longer than its session allows, is dropped and comes back.
fn illustration() -> Markup {
    let ev = |tone: &str, at: &str, cause: &str, head: &str, line: &str| {
        html! {
            div class={"ev " (tone)} {
                div class="tm" { (at) } div class="dot" { i {} }
                div class="tx" { div { span class="cause" { (cause) } b { (head) } } div class="dim" style="margin-top:6px" { (line) } }
            }
        }
    };
    html! {
        div class="card" data-extra="new" {
            div class="row sb" style="margin-bottom:6px" { h2 style="margin:0" { "What it looks like when it is" } span class="pill" { "Illustration, not your group" } }
            p class="lede" { "A client pauses longer than its session timeout, is dropped, comes back, and does it again. The gaps are steady." }
            div class="story" data-extra="story" {
                (ev("bad", "+0:00", "Session expired", "Member timed out", "No heartbeat for 10.0 s, its session timeout."))
                (ev("", "+0:04", "Member joined", "Same member is back", "The group rebalances again."))
                (ev("bad", "+0:30", "Session expired", "Member timed out", "30.1 s after the last expiry."))
                (ev("", "+0:34", "Member joined", "Same member is back", "The pattern is named at the third cycle."))
            }
            div class="callout" style="margin-top:8px" {
                (glyph(Glyph::Info, 18, ""))
                div { b { "Where to look." } " The app spends longer between polls than its session allows: slow processing, a pause, a debugger. Raise " span class="mono" { "session.timeout.ms" } " or do less between polls."
                    " Lab will be where to see this happen, by expiring a member's session; it is not built yet." }
            }
        }
    }
}

/// The page.
#[must_use]
pub fn flapping_page(v: &StoryView, base: &Base) -> Markup {
    let h = &v.head;
    let f = &v.flap;
    let back = html! { a class="btn" href=(story_url(base, &h.id, "")) { (group_glyph(GroupGlyph::Back, 16, "")) "Back to the story" } };
    let sentence = html! {
        "A group is flapping when the same member keeps timing out and coming back. " span class="mono" { (h.id) }
        @if f.flapping { " is " b { "flapping" } ": " (f.cycles) " expired-then-joined cycles in the last 10 minutes." }
        @else { " is " b { "not" } " flapping: " (f.expiries) " session " @if f.expiries == 1 { "expiry" } @else { "expiries" } " since the broker started, and a member that leaves cleanly never counts." }
    };
    html! {
        (group_head(h, sentence, back))
        (group_story_tabs(h, base))
        div class="g2" style="align-items:start" {
            div class="card" {
                h2 { "The rule, against " (h.id) }
                p class="lede" { "Three or more expired-then-joined cycles in 10 minutes." }
                (rules(f))
            }
            (illustration())
        }
    }
}
