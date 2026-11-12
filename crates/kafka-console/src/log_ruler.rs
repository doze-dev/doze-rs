//! The ruler of a partition's offsets: where each segment sits, which offsets compaction removed (the hatched gaps), where
//! cleaning got to and where the log starts and ends. Pure geometry from a [`LogView`]; the page draws it.
//!
//! Width is offsets, not bytes: a segment's cell is as wide as the offsets it spans, so a cleaned segment keeps its width and
//! shows what was taken out of it as hatching.

use crate::format::{bytes, number};
use crate::log_words::{expiry, pct};
use crate::model::{LogSegment, LogState, LogView};

/// The gap, in pixels, between two cells (the kit's `.segs{gap:3px}`).
pub const GAP_PX: i64 = 3;
/// A cell narrower than this share of the ruler carries no label and no axis number: there is no room.
const LABEL_SHARE: f64 = 0.06;

/// The colour of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// Cleaned: the latest record of each key.
    Clean,
    /// Not cleaned yet.
    Dirty,
    /// Being written: never cleaned, never deleted.
    Active,
    /// Sealed, in a log that deletes.
    Sealed,
}

impl Cell {
    /// The class of the kit.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Active => "active",
            Self::Sealed => "sealed",
        }
    }
}

/// A marker above the ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub label: String,
    pub offset: i64,
    /// Where along the ruler, 0 to 1.
    pub at: f64,
    /// It is the end of the log (the high watermark and the last stable offset): drawn with the accent.
    pub end: bool,
}

/// One segment's cell.
#[derive(Debug, Clone, PartialEq)]
pub struct RulerCell {
    pub cell: Cell,
    /// Its width in offsets.
    pub flex: i64,
    /// The hatched gaps, as (left, width) in per cent of the cell.
    pub gaps: Vec<(f64, f64)>,
    /// Its two lines of label, where there is room.
    pub label: Option<(String, String)>,
    /// The number under it, where there is room.
    pub axis: Option<String>,
}

/// One entry of the key under the ruler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Cleaned,
    Dirty,
    Active,
    ActiveNever,
    Sealed,
    Hatching,
}

/// The whole ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Ruler {
    pub marks: Vec<Mark>,
    pub cells: Vec<RulerCell>,
    pub key: Vec<Key>,
    /// The first offset the ruler draws.
    pub from: i64,
}

impl Ruler {
    /// The pixels the gaps between cells take, so that a marker's place can allow for them.
    #[must_use]
    pub fn gap_px(&self) -> i64 {
        GAP_PX * i64::try_from(self.cells.len().saturating_sub(1)).unwrap_or(0)
    }
}

/// The CSS custom property and the rest of a marker's style: the place along the cells, allowing for the gaps between them.
#[must_use]
pub fn mark_style(at: f64, gap_px: i64) -> String {
    if at <= 0.0 {
        format!(
            "--at:calc((100% - {gap_px}px) * 0.00000 + 0px);transform:none;align-items:flex-start"
        )
    } else if at >= 1.0 {
        format!(
            "--at:calc((100% - {gap_px}px) * 1.00000 + {gap_px}px);transform:translateX(-100%);align-items:flex-end"
        )
    } else {
        let half = gap_px as f64 / 2.0;
        format!("--at:calc((100% - {gap_px}px) * {at:.5} + {half}px);")
    }
}

fn share(flex: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        flex as f64 / total as f64
    }
}

fn cell_of(v: &LogView, s: &LogSegment) -> Cell {
    if s.active {
        return Cell::Active;
    }
    match &v.compaction {
        Some(c) if s.base < c.planner.dirty_start => Cell::Clean,
        Some(_) => Cell::Dirty,
        None => Cell::Sealed,
    }
}

fn gaps_of(v: &LogView, s: &LogSegment) -> Vec<(f64, f64)> {
    let span = s.span();
    if span <= 0 {
        return Vec::new();
    }
    let mut all: Vec<(i64, i64)> = Vec::new();
    // What the log start has moved past is gone from every reader, whatever the file still holds.
    if v.start > s.base {
        all.push((s.base, (v.start.min(s.next) - s.base).max(0)));
    }
    all.extend(s.gaps.iter().copied().map(|(f, n)| {
        let f2 = f.max(v.start);
        (f2, (f + n - f2).max(0))
    }));
    all.retain(|g| g.1 > 0);
    all.sort_unstable();
    all.iter()
        .map(|(f, n)| {
            let left = (f - s.base) as f64 / span as f64 * 100.0;
            let width = (*n as f64 / span as f64 * 100.0).max(0.3);
            (left, width)
        })
        .collect()
}

fn label_of(v: &LogView, s: &LogSegment, i: usize, cell: Cell) -> (String, String) {
    let first = format!("{} \u{b7} {}", i + 1, bytes(s.bytes));
    let second = match cell {
        Cell::Clean => {
            if s.removed() > 0 && s.span() > 0 {
                format!("kept {}", pct(1.0 - s.removed() as f64 / s.span() as f64))
            } else {
                "clean".to_owned()
            }
        }
        Cell::Dirty => "dirty".to_owned(),
        Cell::Active if v.compaction.is_some() => "never cleaned".to_owned(),
        Cell::Active => "active: never deleted".to_owned(),
        Cell::Sealed => match expiry(v) {
            Some(e) if i < e.going_now => "goes at the next pass".to_owned(),
            _ => "sealed".to_owned(),
        },
    };
    if cell == Cell::Active && v.compaction.is_some() {
        return (format!("{} \u{b7} active", i + 1), second);
    }
    (first, second)
}

fn cells(v: &LogView) -> Vec<RulerCell> {
    let total: i64 = v.segments.iter().map(LogSegment::span).sum::<i64>().max(1);
    v.segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let cell = cell_of(v, s);
            let room = share(s.span(), total) >= LABEL_SHARE;
            RulerCell {
                cell,
                flex: s.span().max(1),
                gaps: gaps_of(v, s),
                label: room.then(|| label_of(v, s, i, cell)),
                axis: room.then(|| number(s.base)),
            }
        })
        .collect()
}

fn mark(label: &str, offset: i64, from: i64, end: i64, is_end: bool) -> Mark {
    let len = (end - from).max(0);
    let at = if len == 0 {
        0.0
    } else {
        ((offset - from) as f64 / len as f64).clamp(0.0, 1.0)
    };
    Mark {
        label: label.to_owned(),
        offset,
        at,
        end: is_end,
    }
}

fn marks(v: &LogView, from: i64) -> Vec<Mark> {
    let mut out = vec![mark(
        if v.state() == LogState::Empty {
            "log start = last stable offset = high watermark"
        } else {
            "log start"
        },
        v.start,
        from,
        v.end,
        false,
    )];
    if v.state() == LogState::Empty {
        return out;
    }
    if let Some(c) = &v.compaction {
        out.push(mark(
            "checkpoint \u{b7} first dirty offset",
            c.planner.first_dirty.max(v.checkpoint),
            from,
            v.end,
            false,
        ));
    } else if let Some(e) = expiry(v) {
        out.push(mark(
            &format!(
                "segment {} starts: the log start after the pass",
                if e.going_now > 0 {
                    e.going_now + 1
                } else {
                    e.next.map_or(0, |n| n.0 + 1) + 1
                }
            ),
            e.new_start,
            from,
            v.end,
            false,
        ));
    }
    let end_label = if v.last_stable < v.end {
        format!(
            "high watermark {} \u{b7} last stable offset {}",
            number(v.end),
            number(v.last_stable)
        )
    } else {
        "last stable offset = high watermark".to_owned()
    };
    out.push(mark(&end_label, v.end, from, v.end, true));
    out
}

fn key(v: &LogView) -> Vec<Key> {
    match v.state() {
        LogState::Empty => vec![Key::ActiveNever],
        LogState::Retention => {
            let mut out = vec![Key::Sealed, Key::Active];
            if v.start > v.segments.first().map_or(0, |s| s.base) {
                out.push(Key::Hatching);
            }
            out
        }
        _ => {
            let mut out = vec![Key::Cleaned, Key::Dirty, Key::ActiveNever];
            if v.segments.iter().any(|s| s.removed() > 0)
                || v.start > v.segments.first().map_or(0, |s| s.base)
            {
                out.push(Key::Hatching);
            }
            out
        }
    }
}

/// The ruler of `v`.
#[must_use]
pub fn ruler(v: &LogView) -> Ruler {
    let from = v.segments.first().map_or(v.start, |s| s.base).min(v.start);
    let mut cells = cells(v);
    if cells.is_empty() {
        cells.push(RulerCell {
            cell: Cell::Active,
            flex: 1,
            gaps: Vec::new(),
            label: Some((
                "1 \u{b7} active segment".to_owned(),
                "0 B, no records".to_owned(),
            )),
            axis: None,
        });
    }
    Ruler {
        marks: marks(v, from),
        cells,
        key: key(v),
        from,
    }
}

#[cfg(test)]
mod tests;
