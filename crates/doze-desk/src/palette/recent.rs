//! What ⌘K remembers: the services opened lately and the commands done from it (Go prefs `recent` and `commands`,
//! with `Prefs.Opened` and `Service.RememberCommand`), so its Recent section is what you did, not only where you went.
//! The app keeps both in the shared prefs file, in Go's shape, so the two apps read each other's.

use serde::{Deserialize, Serialize};

use super::{Doing, Row, key_of};
use crate::view::StackView;

/// How many services are remembered.
pub const RECENT_KEPT: usize = 8;

/// How many commands are remembered.
const COMMANDS_KEPT: usize = 6;

/// A service opened: its stack's key and its name (Go `prefs.Recent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentService {
    pub key: String,
    pub name: String,
}

/// A command done from the palette, as the palette described it (Go `view.RecentCommand`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentCommand {
    pub glyph: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(rename = "do")]
    pub doing: Doing,
}

impl RecentCommand {
    /// The command a row does.
    #[must_use]
    pub fn of(r: &Row) -> RecentCommand {
        RecentCommand {
            glyph: r.glyph.to_owned(),
            label: r.label.clone(),
            note: r.note.clone(),
            doing: r.doing.clone(),
        }
    }
}

/// Puts a service at the front of the list, once, and keeps [`RECENT_KEPT`] (Go `Prefs.Opened`).
#[must_use]
pub fn opened(list: &[RecentService], key: &str, name: &str) -> Vec<RecentService> {
    let first = RecentService {
        key: key.to_owned(),
        name: name.to_owned(),
    };
    std::iter::once(first.clone())
        .chain(list.iter().filter(|r| **r != first).cloned())
        .take(RECENT_KEPT)
        .collect()
}

/// Puts a command at the front, the same label once, six at most (Go `RememberCommand`).
#[must_use]
pub fn remember(list: &[RecentCommand], c: RecentCommand) -> Vec<RecentCommand> {
    if c.label.is_empty() {
        return list.to_vec();
    }
    let label = c.label.clone();
    std::iter::once(c)
        .chain(list.iter().filter(|o| o.label != label).cloned())
        .take(COMMANDS_KEPT)
        .collect()
}

/// The kept commands still meaningful (their stack is still known), as rows, four at most (Go `recentCommands`).
pub(crate) fn recent_commands(stacks: &[&StackView], kept: &[RecentCommand]) -> Vec<Row> {
    kept.iter()
        .filter(|c| {
            c.doing.stack_key.is_empty() || stacks.iter().any(|s| key_of(s) == c.doing.stack_key)
        })
        .take(4)
        .map(|c| Row {
            glyph: glyph(&c.glyph),
            tone: "dim",
            label: c.label.clone(),
            note: c.note.clone(),
            doing: c.doing.clone(),
            ..Row::default()
        })
        .collect()
}

/// The palette's glyphs, so a kept one reads back as the same `&'static str`.
const GLYPHS: [&str; 26] = [
    "dot", "↓", "↑", "↻", "■", "☐", "→", "≡", "◷", "✎", "▤", "›_", "⧉", "⌫", "↗", "▣", "◆", "⊘",
    "⚠", "▸", "⌘O", "⌂", "◉", "⚙", "+", "?",
];

fn glyph(g: &str) -> &'static str {
    GLYPHS
        .iter()
        .chain(["✧", "◐", "●", "⏻", "⌕"].iter())
        .find(|x| **x == g)
        .copied()
        .unwrap_or("›")
}
