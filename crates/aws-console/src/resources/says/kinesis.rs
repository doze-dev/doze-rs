//! Kinesis Data Streams: how a stream's page speaks. Its sentence needs the consumers' names, which the app reads; the header says
//! the state (`Active`, as the board words it) and offers nothing of its own (each setting is its section's button, the deletion is
//! the Actions menu's).

use super::super::{Page, Read};
use super::tone;
use crate::model::Badge;

/// `ACTIVE` as `Active`, with the tone of its status.
fn badges(r: &Read<'_>) -> Vec<Badge> {
    r.text("StreamDescriptionSummary.StreamStatus")
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            let mut chars = lower.chars();
            let shown: String = chars
                .next()
                .map(|c| c.to_ascii_uppercase())
                .into_iter()
                .chain(chars)
                .collect();
            Badge::new(shown, tone(&word))
        })
        .into_iter()
        .collect()
}

/// A stream's header: its status.
pub const STREAM: Page = Page {
    sentence: None,
    badges: Some(badges),
    verbs: &[],
    summary: None,
};
