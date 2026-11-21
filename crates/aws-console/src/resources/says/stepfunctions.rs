//! Step Functions: how a state machine's page speaks. What it says of its runs (how many are running, the last to fail) is read
//! from its executions by the page; the badges and the verbs of the header are the machine's own.

use super::super::{Page, Read, Verb};
use super::status;
use crate::model::{Badge, Tone};

/// What kind of machine it is, and its status: `Standard`, `Express`; `Active`.
fn badges(r: &Read<'_>) -> Vec<Badge> {
    let kind = match r.text("type").as_deref() {
        Some("EXPRESS") => "Express",
        _ => "Standard",
    };
    let mut out = vec![Badge::new(kind, Tone::Neutral)];
    out.extend(status(r.text("status")));
    out
}

/// A machine's header: Edit and Start execution (Delete is in the Actions menu, with a dialog of its own).
pub const MACHINE: Page = Page {
    badges: Some(badges),
    verbs: &[
        Verb::ui("edit", "Edit"),
        Verb::ui("start", "Start execution"),
    ],
    ..Page::NONE
};
