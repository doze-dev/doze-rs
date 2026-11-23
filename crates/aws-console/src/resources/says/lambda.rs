//! Lambda: how a function's page speaks.

use super::super::page::plural;
use super::super::{Page, Read, Verb};
use super::status;
use crate::model::{Badge, ResourceRow, Tone};

/// `send-email runs python3.13 with 256 MB and a 30 second timeout.` The page adds what triggers it and when it last ran, which
/// are not in the answers this reads.
fn sentence(r: &Read<'_>) -> Option<String> {
    let runtime = r.text("Configuration.Runtime");
    let memory = r.number("Configuration.MemorySize")?;
    let timeout = r.number("Configuration.Timeout")?;
    Some(format!(
        "{} runs {}with {memory} MB and a {timeout} second timeout.",
        r.name,
        runtime.map_or_else(String::new, |rt| format!("{rt} "))
    ))
}

/// What it is (a .zip or an image package), and its state.
fn badges(r: &Read<'_>) -> Vec<Badge> {
    let package = match r.text("Configuration.PackageType").as_deref() {
        Some("Image") => "Image",
        _ => "Zip",
    };
    let mut out = vec![Badge::new(package, Tone::Neutral)];
    out.extend(status(r.text("Configuration.State")));
    out
}

/// A function's header: what can be done to it. Delete opens a dialog of its own.
pub const FUNCTION: Page = Page {
    sentence: Some(sentence),
    badges: Some(badges),
    verbs: &[
        Verb::ui("trigger", "Add trigger"),
        Verb::ui("destination", "Add destination"),
        Verb::ui("delete", "Delete").danger(),
    ],
    summary: Some(summary),
};

/// `3 functions run here.`
fn summary(rows: &[ResourceRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let n = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    Some(format!(
        "{} {} here.",
        plural(n, "function", "functions"),
        if n == 1 { "runs" } else { "run" }
    ))
}
