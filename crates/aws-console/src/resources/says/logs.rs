//! CloudWatch Logs: how a log group's page speaks. The sentence is the app's (it counts the last hour's events, which the group's
//! own calls do not give); the header says the class and offers the search.

use super::super::{Page, Read, Verb};
use crate::model::{Badge, Tone};

/// `Standard`, or `Essentials` for the class AWS renamed from Infrequent Access.
fn badges(r: &Read<'_>) -> Vec<Badge> {
    let essentials = r
        .text("logGroups.@logGroupName.logGroupClass")
        .is_some_and(|c| c == "INFREQUENT_ACCESS");
    vec![Badge::new(
        if essentials { "Essentials" } else { "Standard" },
        Tone::Neutral,
    )]
}

/// A log group's header.
pub const GROUP: Page = Page {
    sentence: None,
    badges: Some(badges),
    verbs: &[Verb::ui("search", "Search log group")],
    summary: None,
};
