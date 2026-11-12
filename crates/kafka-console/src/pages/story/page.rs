//! `group-story`: the Story tab of a group, and the page for a group the console cannot show.

use maud::{Markup, html};

use super::cards::{calm, causes, flapping_card, now, quiet};
use super::generation::generation;
use crate::base::Base;
use crate::model::{GroupHead, StoryView};
use crate::pages::groups::{group_head, group_story_tabs, group_switch, group_url};
use crate::story::is_quiet;
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};

/// The Story page's regions (the sentence is still a region until the Story is converted to metric slots).
pub const STORY_SENTENCE: &str = "story-stats";
pub const STORY_NOW: &str = "story-now";

/// `/groups/{group}/story` and what follows it.
#[must_use]
pub fn story_url(base: &Base, id: &str, tail: &str) -> String {
    group_url(base, id, &format!("/story{tail}"))
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

fn fragment_url(base: &Base, id: &str, group: &str) -> String {
    base.url(&format!(
        "/fragments/{id}?group={}",
        crate::base::segment(group)
    ))
}

/// What the ring holds, as a strip of metric slots in a region a Refresh fetches again (CON-805).
#[must_use]
pub fn sentence_fragment(v: &StoryView, base: &Base) -> Markup {
    let forming = v.generations.first().is_some_and(|g| g.forming);
    let newest = if v.generations.is_empty() {
        "none"
    } else if forming {
        "Forming"
    } else {
        "Settled"
    };
    let since = match v.since_ms {
        Some(ms) => crate::ui::metrics::Stat::at("Oldest note", ms, v.head.now_ms),
        None => crate::ui::metrics::Stat::new("Oldest note", "none")
            .mono()
            .ch(11),
    };
    client::fragment(
        STORY_SENTENCE,
        &fragment_url(base, STORY_SENTENCE, &v.head.id),
        crate::ui::metrics::stat_strip(&[
            crate::ui::metrics::Stat::new("Rebalances kept", v.generations.len().to_string()).ch(5),
            crate::ui::metrics::Stat::new("Newest", newest).ch(9),
            crate::ui::metrics::Stat::new("Causes", v.causes.len().to_string()).ch(3),
            since,
        ]),
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
