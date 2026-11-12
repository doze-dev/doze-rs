//! What the pages of one group share: its title row, the switch-group chips, the tabs and the sentence.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::number;
use crate::model::{GroupHead, GroupPhase, GroupProtocol, GroupSpan};
use crate::ui::button::{Size, State, Variant, opener, state};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::kafka::title_row;
use crate::ui::metrics::{Stat, stat_strip};

/// The region that holds a group's facts.
pub const GROUP_STATS: &str = "group-stats";

/// `/groups/{group}` and what follows it.
#[must_use]
pub fn group_url(base: &Base, id: &str, tail: &str) -> String {
    base.url(&format!("/groups/{}{tail}", segment(id)))
}

/// A sentence of spans; a topic is a link to its page.
#[must_use]
pub(super) fn spans(parts: &[GroupSpan], base: &Base) -> Markup {
    html! {
        @for p in parts {
            @match p {
                GroupSpan::Text(t) => (t),
                GroupSpan::Mono(t) => span class="mono" { (t) },
                GroupSpan::Strong(t) => b { (t) },
                GroupSpan::Topic(t) => a class="lnk" href=(base.url(&format!("/topics/{}", segment(t)))) { (t) },
            }
        }
    }
}

/// The colour of a state word.
#[must_use]
pub(crate) fn tone(p: GroupPhase) -> State {
    match p {
        GroupPhase::Stable => State::Ok,
        GroupPhase::Empty => State::Idle,
        _ => State::Warn,
    }
}

/// The pills beside a group's name: the protocol, the assignor, the generation or epoch.
fn pills(h: &GroupHead) -> Markup {
    let generation = match (h.protocol, h.phase) {
        (GroupProtocol::Consumer, _) => Some(format!("epoch {}", h.generation)),
        (_, GroupPhase::Empty) => Some("no members".to_owned()),
        (_, GroupPhase::Rebalancing) => Some(format!("generation {} forming", h.generation + 1)),
        _ => Some(format!("generation {}", h.generation)),
    };
    html! {
        @match h.protocol {
            GroupProtocol::Classic => {
                span class="pill" { "classic" }
                @if let Some(a) = h.assignor.as_ref().filter(|_| h.phase != GroupPhase::Empty) { " " span class="pill" { (a) } }
            },
            GroupProtocol::Consumer => { span class="pill" { "consumer protocol (KIP-848)" } },
        }
        @if let Some(g) = generation { " " span class="pill" { (g) } }
    }
}

/// The title row: the name, its state, and the pills.
#[must_use]
pub(super) fn title(h: &GroupHead) -> Markup {
    title_row(
        &h.id,
        html! { (state(tone(h.phase), h.phase.word())) " " (pills(h)) },
    )
}

/// The page head: title row, the actions at the right, and under them what follows the group (a strip of metric slots, or a
/// static line for a page that has none).
#[must_use]
pub(crate) fn head(h: &GroupHead, under: Markup, acts: Markup) -> Markup {
    html! {
        div class="page-h" {
            div { (title(h)) }
            div class="acts" { (acts) }
        }
        (under)
    }
}

/// What follows the group, as a strip of metric slots in a region a Refresh fetches again (CON-805).
#[must_use]
pub fn stats_fragment(h: &GroupHead, base: &Base) -> Markup {
    let n = |v: usize| number(i64::try_from(v).unwrap_or(0));
    let generation = match h.protocol {
        GroupProtocol::Consumer => ("Epoch", h.generation.to_string()),
        GroupProtocol::Classic => ("Generation", h.generation.to_string()),
    };
    let last = match h.last_commit_ms {
        Some(ms) => Stat::at("Last commit", ms, h.now_ms),
        None => Stat::new("Last commit", "never").mono().ch(11),
    };
    client::fragment(
        GROUP_STATS,
        &base.url(&format!(
            "/fragments/{GROUP_STATS}?group={}",
            segment(&h.id)
        )),
        stat_strip(&[
            Stat::new("State", h.phase.word()).ch(12),
            Stat::new("Members", n(h.members.len())).ch(4),
            Stat::new(generation.0, generation.1).ch(6),
            Stat::new("Lag", number(h.lag_total)).ch(9),
            Stat::new("Committed partitions", n(h.commits)).ch(4),
            last,
        ]),
    )
}

/// The chips that switch to another group's same page.
#[must_use]
pub(crate) fn switch(h: &GroupHead, base: &Base, tail: &str) -> Markup {
    html! {
        div class="toolbar" style="margin:4px 0 28px" {
            span class="dim" { (group_glyph(GroupGlyph::Group, 16, "")) " Switch group" }
            @for g in &h.groups {
                @if *g == h.id {
                    span class="fchip on mono" { (g) }
                } @else {
                    a class="fchip mono" href=(group_url(base, g, tail)) { (g) }
                }
            }
        }
    }
}

/// Which tab is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum On {
    Members,
    Lag,
    Story,
}

/// The tabs: Members, Lag, and Story, whose count is the rebalances the group has said it began (the ring of slice 9f).
#[must_use]
pub(super) fn group_tabs(h: &GroupHead, base: &Base, on: On) -> Markup {
    let lag = i64_usize(h.lag_total);
    html! {
        div class="tabs" {
            @for (label, href, here, count) in [
                ("Members", group_url(base, &h.id, ""), on == On::Members, h.members.len()),
                ("Lag", group_url(base, &h.id, "/lag"), on == On::Lag, lag),
                ("Story", group_url(base, &h.id, "/story"), on == On::Story, h.story.count),
            ] {
                @if here {
                    a class="tab on" href=(href) aria-current="page" { (label) span class="n" { (crate::format::number(i64::try_from(count).unwrap_or(0))) } }
                } @else {
                    a class="tab" href=(href) { (label) span class="n" { (crate::format::number(i64::try_from(count).unwrap_or(0))) } }
                }
            }
        }
    }
}

fn i64_usize(n: i64) -> usize {
    usize::try_from(n.max(0)).unwrap_or(usize::MAX)
}

/// The tabs with Story showing: for the Story pages, which sit beside the group's own.
#[must_use]
pub(crate) fn story_tabs(h: &GroupHead, base: &Base) -> Markup {
    group_tabs(h, base, On::Story)
}

/// The two buttons most group pages carry.
#[must_use]
pub(super) fn reset_and_delete(h: &GroupHead, base: &Base) -> Markup {
    html! {
        a class="btn" href=(group_url(base, &h.id, "/reset")) data-dialog data-op="OffsetCommit" { (crate::ui::glyph::glyph(Glyph::Refresh, 16, "")) "Reset offsets" }
        (opener(&group_url(base, &h.id, "/delete"), Variant::Danger, Size::Normal, Some(Glyph::Trash), "Delete group"))
    }
}
