//! The ruler section: the heading, the ruler of offsets with the hatched gaps, and the two paragraphs that say what the
//! gaps and the log start mean.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::number;
use crate::log_ruler::{Cell, Key, Ruler, RulerCell, mark_style, ruler};
use crate::model::{Cleanup, LogState, LogView};

fn marks(r: &Ruler) -> Markup {
    let gap = r.gap_px();
    html! {
        div class="mks" style="height:66px" {
            @for m in &r.marks {
                div class={"mk" @if m.end { " lso hw" } @else { " " }} style=(mark_style(m.at, gap)) {
                    span { (m.label) } b { (number(m.offset)) }
                }
            }
        }
    }
}

fn cell(c: &RulerCell) -> Markup {
    let bare = c.label.is_none() && c.cell == Cell::Active;
    html! {
        div class={"sg " (c.cell.class())} style={"flex:" (c.flex) ";" @if bare { "padding:0;justify-content:center;" }} {
            @for (at, w) in &c.gaps { i class="gp" style={"--at:" (format!("{at:.2}")) "%;--w:" (format!("{w:.2}")) "%"} {} }
            @if let Some((a, b)) = &c.label { span { (a) br; (b) } }
        }
    }
}

fn axis(r: &Ruler) -> Markup {
    html! {
        div class="axis" style="gap:3px;justify-content:flex-start" {
            @for (i, c) in r.cells.iter().enumerate() {
                @if i + 1 == r.cells.len() && c.cell == Cell::Active && c.axis.is_some() {
                    span style={"flex:" (c.flex) ";display:flex;justify-content:flex-end"} {
                        span style="white-space:nowrap" { (c.axis.clone().unwrap_or_default()) }
                    }
                } @else {
                    span style={"flex:" (c.flex)} { (c.axis.clone().unwrap_or_default()) }
                }
            }
        }
    }
}

fn key_item(k: Key) -> Markup {
    let swatch = |style: &str, text: &str| html! { span { i style=(style) {} (text) } };
    match k {
        Key::Cleaned => swatch(
            "background:#e8f4ef;border-color:#b9dccb;",
            "cleaned: the latest record of each key",
        ),
        Key::Dirty => swatch(
            "background:#fdf3e1;border-color:#ecd9a8;",
            "dirty: not cleaned yet",
        ),
        Key::Active => swatch("background:#ece6fb;border-color:#cdbff4;", "active"),
        Key::ActiveNever => swatch(
            "background:#ece6fb;border-color:#cdbff4;",
            "active: never cleaned",
        ),
        Key::Sealed => swatch("background:#eef3fd;border-color:#c9d8f6;", "sealed"),
        Key::Hatching => swatch(
            "background:repeating-linear-gradient(135deg,rgba(23,20,31,0) 0 3px,rgba(23,20,31,.28) 3px 4px);border-color:var(--line2)",
            "hatching: records compaction removed",
        ),
    }
}

fn key(v: &LogView, r: &Ruler) -> Markup {
    let note = match v.state() {
        LogState::Empty => "Nothing to compact, nothing to delete",
        _ => "Width is offsets, not bytes",
    };
    html! {
        div class="key" {
            @for k in &r.key {
                @if *k == Key::ActiveNever && v.state() == LogState::Retention { (key_item(Key::Active)) } @else { (key_item(*k)) }
            }
            span { span { (note) } }
        }
    }
}

/// The paragraph about gaps, with a real one as its example.
fn gaps_help(v: &LogView) -> Markup {
    let first = v
        .segments
        .iter()
        .flat_map(|s| s.gaps.iter())
        .find(|(f, n)| *f >= v.start && f + n < v.end);
    html! {
        p class="help" style="margin:16px 0 0;max-width:none" {
            b { "Gaps." } " Hatched: offsets that cleaning removed. Offsets never move, so a read from inside one carries on at the next record"
            @if let Some((f, n)) = first {
                ": a Fetch at " span class="mono" { (number(*f)) } " returns the record at " span class="mono" { (number(f + n)) } ", with no error"
            }
            ". A cleaned batch keeps the offsets it spans, so the last offset of a segment is never lost."
        }
    }
}

fn start_help(v: &LogView, base: &Base) -> Markup {
    let delete = base.url(&format!("/topics/{}/delete-records", segment(&v.topic)));
    let only_compacts = v.cleanup == Cleanup::Compact;
    html! {
        p class="help" style="margin:8px 0 0;max-width:none" {
            b { "Log start." } " Delete records moves it up, and everything below goes"
            @if only_compacts {
                " (it is " a class="lnk" href=(delete) { "refused on this compacted topic" } ": " span class="mono" { "POLICY_VIOLATION (44)" } ")"
            } @else {
                " (the Delete records dialog of the topic does it)"
            }
            ". A Fetch below the start gets " span class="mono" { "OFFSET_OUT_OF_RANGE (1)" }
            ", and the last stable offset is never below the start."
            @if v.start > 0 { " Here the log start is " span class="mono" { (number(v.start)) } ": the offsets below it are hatched." }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn section(v: &LogView, base: &Base) -> Markup {
    let r = ruler(v);
    let empty = v.state() == LogState::Empty;
    html! {
        div class="sec" {
            @if !empty {
                h2 { "The log, offset by offset" }
                p class="lede" { "Every segment of partition " (v.partition) " in offset order, as " @if v.compaction.is_some() { "the cleaner" } @else { "retention" } " sees it. Open a row of the segments table below for its files and index." }
            }
            div class="ruler" data-extra="ruler" {
                (marks(&r))
                div class="segs" style="height:80px" { @for c in &r.cells { (cell(c)) } }
                (axis(&r))
                (key(v, &r))
                @if !empty {
                    @if v.compaction.is_some() { (gaps_help(v)) }
                    (start_help(v, base))
                }
            }
        }
    }
}
