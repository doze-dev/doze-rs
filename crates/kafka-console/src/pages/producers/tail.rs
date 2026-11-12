//! The tail of a partition drawn offset by offset: which records a `read_committed` consumer may read, where the markers are, and
//! which are held back by an open transaction.

use maud::{Markup, html};

use crate::format::number;
use crate::log_ruler::GAP_PX;
use crate::model::{ProdCell, ProdTail};

/// The style of a marker above the tail: at the left edge of cell `index` of `n`, allowing for the gaps between cells.
#[must_use]
pub(super) fn mark_style(index: usize, n: usize) -> String {
    let gap = GAP_PX * i64::try_from(n.saturating_sub(1)).unwrap_or(0);
    if index >= n {
        return format!(
            "--at:calc((100% - {gap}px) * 1.00000 + {gap}px);transform:translateX(-100%);align-items:flex-end"
        );
    }
    let share = index as f64 / n.max(1) as f64;
    let px = GAP_PX as f64 * index as f64 - GAP_PX as f64 / 2.0;
    format!("--at:calc((100% - {gap}px) * {share:.5} + {px}px);")
}

fn class(c: ProdCell) -> &'static str {
    match c {
        ProdCell::Committed => "clean",
        ProdCell::Marker => "sealed",
        ProdCell::Held => "active",
    }
}

/// The ruler of the tail up to `end`, with the last stable offset marked and the held-back cells counted.
#[must_use]
pub(super) fn ruler(t: &ProdTail, last_stable: i64, end: i64) -> Markup {
    let n = t.cells.len();
    let held = t.cells.iter().filter(|c| **c == ProdCell::Held).count();
    let stable_index = usize::try_from((last_stable - t.from).max(0))
        .unwrap_or(0)
        .min(n);
    html! {
        div class="ruler" data-extra="ruler" {
            div class="mks" style="height:66px" {
                @if held > 0 { div class="mk lso" style=(mark_style(stable_index, n)) { span { "last stable offset" } b { (number(last_stable)) } } }
                div class="mk hw" style=(mark_style(n, n)) { span { "high watermark" } b { (number(end)) } }
            }
            div class="segs" style="height:56px" {
                @for (i, c) in t.cells.iter().enumerate() {
                    div class={"sg " (class(*c))} style="flex:1;padding:0;justify-content:center;" { span { (number(t.from + i64::try_from(i).unwrap_or(0))) } }
                }
            }
            div class="key" {
                span { i style="background:#e8f4ef;border-color:#b9dccb;" {} "committed records" }
                span { i style="background:#eef3fd;border-color:#c9d8f6;" {} "a commit marker, a control record that takes an offset" }
                @if held > 0 { span { i style="background:#ece6fb;border-color:#cdbff4;" {} "in the open transaction: " (held) " records a read_committed consumer cannot see" } }
            }
        }
    }
}
