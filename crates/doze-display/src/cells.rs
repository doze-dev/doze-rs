//! What each cell of a module's console says, how a rate is worked out from totals, and which row a detail block
//! follows.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Instant;

use crate::console::{CELL_RUNTIME, Column, ConsoleRow, ConsoleSpec, Node, NodeKind, Role};
use crate::format::{mb, thousands};
use crate::gonum::{fixed, lowercase, parse_float};
use crate::look::Tone;
use crate::tabs::console_body;

/// The last total seen for a rate column, and when.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateSample {
    /// The total.
    pub v: f64,
    /// When it was read.
    pub at: Instant,
}

/// Works out each rate column of `rows` from successive totals, as the module contract says: the module sends a
/// running total and the surface the speed. `prev` holds the last totals by panel, row and column, and is updated;
/// each row gains a `_rate:<key>` cell for each rate that could be worked out. (Go: `Rates`, which returned new
/// rows; these are written in place.)
pub fn rates(
    spec: Option<&ConsoleSpec>,
    panel: &str,
    rows: &mut [ConsoleRow],
    prev: &mut HashMap<String, RateSample>,
    now: Instant,
) {
    let Some(p) = spec.and_then(|s| s.panel(panel)) else {
        return;
    };
    let first = p.first_col();
    for row in rows {
        for col in p.columns.iter().filter(|c| c.role == Role::Rate) {
            let Some(v) = figure(row.cell(&col.key)) else {
                continue;
            };
            let key = format!("{panel}/{first}={}/{}", row.cell(first), col.key);
            if let Some(s) = prev.get(&key).filter(|s| now > s.at && v >= s.v) {
                let per = (v - s.v) / now.duration_since(s.at).as_secs_f64();
                row.cells
                    .insert(format!("_rate:{}", col.key), format!("{}/s", fixed(per, 0)));
            }
            prev.insert(key, RateSample { v, at: now });
        }
    }
}

/// A figure as a module writes one, commas and all.
fn figure(raw: &str) -> Option<f64> {
    if raw.contains(',') {
        parse_float(&raw.replace(',', ""))
    } else {
        parse_float(raw)
    }
}

#[allow(clippy::cast_possible_truncation)] // WHY: Go's int64(f), which saturates on the platforms doze ships for
fn whole(f: f64) -> i64 {
    f as i64
}

/// A cell's text and tone, by what the module said the column is: a count with commas, bytes as a size, a rate or
/// a dash while there is none, a lag that is green at zero and red above it, a state with its glyph. A row the
/// module marked dim is dim except for its state; an empty "from" column says doze.hcl, or runtime for a row made
/// while the stack ran. (Go: `Cell`.)
#[must_use]
pub fn cell<'a>(col: &Column, row: &'a ConsoleRow) -> (Cow<'a, str>, Tone) {
    let raw = row.cell(&col.key);
    let (mut text, mut tone) = (Cow::Borrowed(raw), Tone::Text);
    match col.role {
        Role::Count => {
            if let Some(n) = figure(raw) {
                text = Cow::Owned(thousands(whole(n)));
            }
        }
        Role::Bytes => {
            if let Ok(n) = raw.parse::<i64>() {
                text = Cow::Owned(mb(n));
            }
        }
        Role::Rate => {
            text = Cow::Borrowed(row.cell(&format!("_rate:{}", col.key)));
            if text.is_empty() {
                (text, tone) = (Cow::Borrowed("—"), Tone::Dim);
            }
        }
        Role::Lag => {
            if let Some(n) = figure(raw) {
                text = Cow::Owned(thousands(whole(n)));
                tone = if n == 0.0 { Tone::Good } else { Tone::Bad };
            }
        }
        Role::State => {
            let (glyph, t) = state_glyph(&lowercase(raw));
            (text, tone) = (Cow::Owned(format!("{glyph} {raw}")), t);
        }
        Role::Text => {}
    }
    if row.tone == "dim" && col.role != Role::State {
        tone = Tone::Dim;
    }
    if col.key == "from" && col.title == "from" && raw.is_empty() {
        (text, tone) = if row.cell(CELL_RUNTIME).is_empty() {
            (Cow::Borrowed("doze.hcl"), Tone::Dim)
        } else {
            (Cow::Borrowed("runtime"), Tone::Warn)
        };
    }
    (text, tone)
}

fn state_glyph(word: &str) -> (&'static str, Tone) {
    match word {
        "ok" | "active" | "stable" | "awake" | "running" | "ready" => ("●", Tone::Good),
        "warn" | "waiting" | "preparing" | "rebalancing" => ("◐", Tone::Warn),
        "bad" | "error" | "failed" | "dead" => ("✗", Tone::Bad),
        _ => ("·", Tone::Dim),
    }
}

/// The key and value pairs of a detail block `n`: the columns of the selected row of the table it follows, or of
/// the first row of its own panel. `tables` are the rows last read, by panel id; `selected` the selected row, by
/// block id; `tab_id` the tab being drawn, whose layout names the table followed. (Go: `DetailRows`.)
#[must_use]
pub fn detail_rows<'a>(
    spec: Option<&'a ConsoleSpec>,
    tab_id: &str,
    tables: &'a HashMap<String, Vec<ConsoleRow>>,
    selected: &HashMap<String, usize>,
    n: &'a Node,
) -> Vec<(&'a str, Cow<'a, str>)> {
    let Some((panel, row)) = followed(spec, tab_id, tables, selected, n) else {
        return Vec::new();
    };
    let Some(p) = spec.and_then(|s| s.panel(panel)) else {
        return Vec::new();
    };
    p.columns
        .iter()
        .map(|col| (col.title.as_str(), cell(col, row).0))
        .collect()
}

/// The panel a detail block draws and the row it shows.
fn followed<'a>(
    spec: Option<&'a ConsoleSpec>,
    tab_id: &str,
    tables: &'a HashMap<String, Vec<ConsoleRow>>,
    selected: &HashMap<String, usize>,
    n: &'a Node,
) -> Option<(&'a str, &'a ConsoleRow)> {
    if n.from.is_empty() {
        return Some((n.panel.as_str(), tables.get(&n.panel)?.first()?));
    }
    let body = console_body(spec, tab_id)?;
    let src = body.walk().find(|x| {
        x.kind == NodeKind::Table && (x.block_id() == n.from || x.panel == n.from || x.id == n.from)
    })?;
    let rows = tables.get(&src.panel)?;
    let i = selected
        .get(src.block_id().as_ref())
        .copied()
        .unwrap_or(0)
        .min(rows.len().checked_sub(1)?);
    let (key, _) = tables.get_key_value(&src.panel)?;
    let panel = if n.panel.is_empty() {
        key.as_str()
    } else {
        n.panel.as_str()
    };
    Some((panel, rows.get(i)?))
}

/// The key that runs a module's action: the first letter of its id, capitalised. (Go: `ActionKey`.)
#[must_use]
pub fn action_key(id: &str) -> Option<char> {
    id.chars().next().map(crate::gonum::upper)
}
