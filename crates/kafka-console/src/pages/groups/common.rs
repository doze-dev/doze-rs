//! What the pages of one group share: its title row, the switch-group chips, the tabs and the sentence.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::model::{GroupHead, GroupPhase, GroupProtocol, GroupSpan};
use crate::ui::button::{Size, State, Variant, opener, state};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::kafka::title_row;

/// The fragment that holds the sentence under a group's name.
pub const GROUP_SENTENCE: &str = "group-sentence";

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

/// The page head: title row, a sentence under it, and the actions at the right.
#[must_use]
pub(super) fn head(h: &GroupHead, sentence: Markup, acts: Markup) -> Markup {
    html! {
        div class="page-h" {
            div { (title(h)) div class="sub" { (sentence) } }
            div class="acts" { (acts) }
        }
    }
}

/// The sentence in its fragment, so that it follows the group while the page is open.
#[must_use]
pub fn sentence_fragment(
    h: &GroupHead,
    base: &Base,
    sentence: &[GroupSpan],
    extra: bool,
) -> Markup {
    let url = base.url(&format!(
        "/fragments/{GROUP_SENTENCE}?group={}",
        segment(&h.id)
    ));
    client::fragment(
        GROUP_SENTENCE,
        &url,
        html! { span data-extra=[extra.then_some("live-sentence")] { (spans(sentence, base)) } },
    )
}

/// The chips that switch to another group's same page.
#[must_use]
pub(super) fn switch(h: &GroupHead, base: &Base, tail: &str) -> Markup {
    html! {
        div class="toolbar" style="margin:-12px 0 28px" {
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
}

/// The tabs. Story is drawn as the boards draw it and is not a link until the group event ring exists (9f).
#[must_use]
pub(super) fn group_tabs(h: &GroupHead, base: &Base, on: On) -> Markup {
    let lag = i64_usize(h.lag_total);
    html! {
        div class="tabs" {
            @for (label, href, here, count) in [
                ("Members", group_url(base, &h.id, ""), on == On::Members, h.members.len()),
                ("Lag", group_url(base, &h.id, "/lag"), on == On::Lag, lag),
            ] {
                @if here {
                    a class="tab on" href=(href) aria-current="page" { (label) span class="n" { (crate::format::number(i64::try_from(count).unwrap_or(0))) } }
                } @else {
                    a class="tab" href=(href) { (label) span class="n" { (crate::format::number(i64::try_from(count).unwrap_or(0))) } }
                }
            }
            span class="tab" title="The story of a group needs the group event ring (slice 9f)" { "Story" }
        }
    }
}

fn i64_usize(n: i64) -> usize {
    usize::try_from(n.max(0)).unwrap_or(usize::MAX)
}

/// The two buttons most group pages carry.
#[must_use]
pub(super) fn reset_and_delete(h: &GroupHead, base: &Base) -> Markup {
    html! {
        a class="btn" href=(group_url(base, &h.id, "/reset")) data-dialog data-op="OffsetCommit" { (crate::ui::glyph::glyph(Glyph::Refresh, 16, "")) "Reset offsets" }
        (opener(&group_url(base, &h.id, "/delete"), Variant::Danger, Size::Normal, Some(Glyph::Trash), "Delete group"))
    }
}
