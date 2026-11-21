//! A card's state as every other screen draws it. The mark is the shared one (`instance::ui::state_mark`, drawn by
//! `cards::state_dot`); here is the frame that goes with it and the memory on the top row. A failed one has a red frame,
//! dashed, and no fill (doze stopped trying; it waits for you); off has a dashed frame and no fill; paused a dashed
//! frame; crashing a red frame and a red tint.

use doze_display::Look;
use gpui::Hsla;

use crate::model::Row;
use crate::theme::{Tokens, mix};

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

/// What the top row's right says (Go `r.mem`): the memory of one awake now, else nothing.
pub(crate) fn mem(r: Option<&Row>) -> Option<String> {
    r.filter(|r| r.awake && !r.mem.is_empty())
        .map(|r| r.mem.clone())
}
