//! A card's state as every other screen draws it: one mark per look (`doze_display::Look`), and the frame that goes
//! with it. ● awake, ◐ dozing, a ring waking or running, a dim dot asleep, a dim ring off, ⊘ paused, ✗ crashing or
//! failed, ✓ done, ▸ not run. A failed one has a red frame, dashed, and no fill (doze stopped trying; it waits for
//! you); off has a dashed frame and no fill; paused a dashed frame; crashing a red frame and a red tint.
//!
//! TODO(#23, app-states): this mirrors `instance::ui::state_mark` and the tiles' frame rules from that branch; once
//! both land, call `state_mark` here instead of [`mark`].

use doze_display::Look;
use gpui::{AnyElement, Hsla, IntoElement, Styled, div, prelude::*, relative};

use crate::model::Row;
use crate::theme::{Tokens, mix};

/// The mark for a look; `trouble` (in trouble for a reason the look does not say) is a red dot.
pub(crate) fn mark(
    t: &Tokens,
    look: Option<Look>,
    trouble: bool,
    size: gpui::Pixels,
) -> AnyElement {
    let disc = |c: Hsla| div().size(size).flex_none().rounded_full().bg(c);
    let ring = |c: Hsla| {
        div()
            .size(size)
            .flex_none()
            .rounded_full()
            .border(size / 4.)
            .border_color(c)
    };
    let glyph = |l: Look| {
        div()
            .w(size + gpui::px(2.))
            .flex_none()
            .flex()
            .justify_center()
            .text_size(size + gpui::px(4.))
            .line_height(size + gpui::px(2.))
            .text_color(t.tone(l.tone()))
            .child(l.glyph())
            .into_any_element()
    };
    match look {
        Some(l @ (Look::Crashing | Look::Failed)) => glyph(l),
        _ if trouble => disc(t.bad).into_any_element(),
        Some(Look::Awake) => disc(t.good).into_any_element(),
        // Half filled, as the ◐ the words use.
        Some(Look::Dozing) => ring(t.warn)
            .overflow_hidden()
            .child(div().h_full().w(relative(0.5)).bg(t.warn))
            .into_any_element(),
        Some(Look::Waking | Look::Running) => ring(t.cool).into_any_element(),
        Some(Look::Off) => div()
            .size(size)
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(t.dim)
            .into_any_element(),
        Some(Look::Asleep) | None => disc(t.dim).into_any_element(),
        Some(l) => glyph(l),
    }
}

/// How a card's frame is drawn for its state.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    pub(crate) border: Hsla,
    pub(crate) fill: Hsla,
    pub(crate) dashed: bool,
}

/// The frame for a card's row (a task's card is dashed whatever its state).
pub(crate) fn frame(t: &Tokens, r: Option<&Row>, task: bool) -> Frame {
    let red = mix(t.bad, t.line, 55.);
    let none = gpui::transparent_black();
    match r.map(|r| (r.look, r.trouble)) {
        Some((Look::Failed, _)) => Frame {
            border: red,
            fill: none,
            dashed: true,
        },
        Some((Look::Off, _)) => Frame {
            border: t.line,
            fill: none,
            dashed: true,
        },
        Some((_, true)) => Frame {
            border: red,
            fill: mix(t.bad, t.panel, 7.),
            dashed: task,
        },
        Some((Look::Paused, _)) => Frame {
            border: t.line,
            fill: t.panel,
            dashed: true,
        },
        _ => Frame {
            border: t.line,
            fill: t.panel,
            dashed: task,
        },
    }
}

/// What the top row's right says: an awake one's memory, else its state's word in its tone.
pub(crate) fn word(t: &Tokens, r: Option<&Row>) -> Option<(String, Hsla)> {
    let r = r?;
    if r.look == Look::Awake {
        return Some((r.mem.clone(), t.dim)).filter(|(m, _)| !m.is_empty() && m != "0");
    }
    Some((r.look.word().to_string(), t.tone(r.look.tone())))
}
