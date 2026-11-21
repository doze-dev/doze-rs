//! Kinesis Data Streams: how a stream's page speaks. Its sentence needs the consumers' names, which the app reads; the header says
//! the state and what can be done to it.

use super::super::{Page, Read};
use crate::model::Badge;
use crate::resources::says::stream;

fn badges(r: &Read<'_>) -> Vec<Badge> {
    stream(r)
}

/// A stream's header: its status (each setting is its section's own button; the deletion is the Actions menu's).
pub const STREAM: Page = Page {
    sentence: None,
    badges: Some(badges),
    verbs: &[],
    summary: None,
};
