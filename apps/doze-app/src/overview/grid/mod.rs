//! The Grid view (the MapGrid board): the stack as a table that reads the same at thirty services as at five, and
//! stays usable at thousands. A row needs each column it has a dot in. Hovering a cell (or moving to it with the
//! arrows) lights its row and its column, and a tooltip under it says what the dot means, why it matters and the line
//! of doze.hcl that makes it so.
//!
//! The table scrolls both ways under a sticky head (the groups and the columns) and a sticky service column, so a
//! row's name and a column's name stay in sight however far it scrolls. Only the rows and columns in view are built:
//! the window's size bounds what can show, so a 2,000-service Grid builds a screenful, not four million cells. The
//! notes under the table sit outside the scroll, always whole.

mod parts;

use std::ops::Range;

use doze_desk::overview::{GridCol, GridRow, GridView};
use gpui::{AnyElement, Context, IntoElement, ScrollEvent, Window, canvas, div, prelude::*, px};

use super::OverviewView;
use super::tiles::filter;
use crate::kit::s;
use crate::theme::Tokens;
use crate::widgets::tracked;

const HEAD_W: f32 = 300.;
pub(super) const ROW_H: f32 = 42.;

/// Where the Grid is scrolled to, and the size of the part that scrolls (measured as it is drawn).
#[derive(Debug, Default)]
pub(crate) struct Scroll {
    pub(crate) x: f32,
    pub(crate) y: f32,
    port: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
}

fn col_w(c: &GridCol) -> f32 {
    match c.kind {
        "app" => 74.,
        "shared" => 68.,
        _ => 136.,
    }
}

/// Each column's left edge, then the table's width.
fn edges(cols: &[GridCol]) -> Vec<f32> {
    let mut at = 0.;
    let mut out = Vec::with_capacity(cols.len() + 1);
    out.push(0.);
    for c in cols {
        at += col_w(c);
        out.push(at);
    }
    out
}

/// The columns that cross `from .. from + width`.
fn cols_in(edges: &[f32], from: f32, width: f32) -> Range<usize> {
    let n = edges.len().saturating_sub(1);
    let start = edges
        .partition_point(|e| *e <= from)
        .saturating_sub(1)
        .min(n);
    let end = edges.partition_point(|e| *e < from + width).min(n);
    start..end.max(start)
}

/// The scrolled size: the rows and the "needed by" line, the columns.
fn extent(v: &OverviewView) -> (f32, f32) {
    let w = edges(&v.home.grid.cols).last().copied().unwrap_or(0.);
    (w, (shown_rows(v).len() + 1) as f32 * ROW_H)
}

/// Keeps the scroll inside the table.
fn clamp(v: &mut OverviewView) {
    let (w, h) = extent(v);
    let (pw, ph) = v.grid.port.get();
    v.grid.x = v.grid.x.clamp(0., (w - pw).max(0.));
    v.grid.y = v.grid.y.clamp(0., (h - ph).max(0.));
}

/// Moves the crosshair with the arrows, scrolling it into view.
pub(super) fn step(v: &mut OverviewView, dx: isize, dy: isize, cx: &mut Context<OverviewView>) {
    let rows = shown_rows(v).len();
    let cols = v.home.grid.cols.len();
    if rows == 0 || cols == 0 {
        return;
    }
    let (r, c) = v.cur.unwrap_or((0, 0));
    let r = r.saturating_add_signed(dy).min(rows - 1);
    let c = c.saturating_add_signed(dx).min(cols - 1);
    v.cur = Some((r, c));
    let (pw, ph) = v.grid.port.get();
    let top = r as f32 * ROW_H;
    v.grid.y = v.grid.y.min(top).max(top + ROW_H - ph);
    let e = edges(&v.home.grid.cols);
    if let (Some(l), Some(rt)) = (e.get(c).copied(), e.get(c + 1).copied()) {
        v.grid.x = v.grid.x.min(l).max(rt - pw);
    }
    clamp(v);
    if let Some(name) = shown_rows(v).get(r).map(|row| row.name.clone()) {
        v.select(&name, cx);
    }
    cx.notify();
}

fn shown_rows(v: &OverviewView) -> Vec<&GridRow> {
    let shown: std::collections::HashSet<&str> =
        v.shown().iter().map(|r| r.name.as_str()).collect();
    v.home
        .grid
        .rows
        .iter()
        .filter(|r| shown.contains(r.name.as_str()))
        .collect()
}

fn on_wheel(
    this: &mut OverviewView,
    ev: &ScrollEvent,
    window: &mut Window,
    cx: &mut Context<OverviewView>,
) {
    let d = ev.delta.pixel_delta(window.line_height());
    let (x, y) = (this.grid.x, this.grid.y);
    this.grid.x -= f32::from(d.x);
    this.grid.y -= f32::from(d.y);
    clamp(this);
    if (x, y) != (this.grid.x, this.grid.y) {
        cx.stop_propagation();
        cx.notify();
    }
}

/// The view: the filter (once asked for), the table, what it cannot show yet.
pub(super) fn view(
    t: &Tokens,
    v: &mut OverviewView,
    window: &Window,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    clamp(v);
    let size = window.viewport_size();
    // What can show is at most the window: enough rows and columns are built to fill it, wherever the box sits.
    let (win_w, win_h) = (f32::from(size.width), f32::from(size.height));
    let e = edges(&v.home.grid.cols);
    let cols = cols_in(&e, v.grid.x, win_w);
    let head = head(t, v, &e, cols.clone());
    let table = table(t, v, &e, cols, win_h, cx);
    let boxed = div()
        .id("gridw")
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .mx(px(28.))
        .mb(px(14.))
        .rounded(px(12.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .overflow_hidden()
        .on_scroll(cx.listener(on_wheel))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered && this.cur.is_some() {
                this.cur = None;
                cx.notify();
            }
        }))
        .child(head)
        .child(table)
        .children(notes(t, &v.home.grid));
    let mut out = div()
        .key_context("Grid")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0();
    if !v.query.is_empty() {
        out = out.child(filter(t, v));
    }
    out.child(boxed)
}

/// The part that scrolls: the sticky service column beside the cells, both moved by the scroll.
fn table(
    t: &Tokens,
    v: &OverviewView,
    e: &[f32],
    cols: Range<usize>,
    win_h: f32,
    cx: &mut Context<OverviewView>,
) -> AnyElement {
    let n_rows = shown_rows(v).len() + 1;
    let first = ((v.grid.y / ROW_H).floor() as usize).min(n_rows);
    let last = (((v.grid.y + win_h) / ROW_H).ceil() as usize + 1).min(n_rows);
    let left = e.get(cols.start).copied().unwrap_or(0.);
    let dy = first as f32 * ROW_H - v.grid.y;
    let (names, cells) = body(t, v, first..last, cols, cx);
    let port = v.grid.port.clone();
    let measure = canvas(
        move |b, _, _| port.set((f32::from(b.size.width), f32::from(b.size.height))),
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    div()
        .flex()
        .flex_1()
        .min_h_0()
        .child(
            div()
                .w(px(HEAD_W))
                .flex_none()
                .overflow_hidden()
                .child(div().flex().flex_col().mt(px(dy)).children(names)),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(measure)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .mt(px(dy))
                        .ml(px(left - v.grid.x))
                        .children(cells),
                ),
        )
        .into_any_element()
}

/// The sticky head: the service column's caption, then the groups' captions and the columns' heads, scrolled across
/// with the table.
fn head(t: &Tokens, v: &OverviewView, e: &[f32], cols: Range<usize>) -> impl IntoElement {
    let g = &v.home.grid;
    let left = e.get(cols.start).copied().unwrap_or(0.);
    let mut groups = div().flex();
    let mut at = 0;
    for h in &g.heads {
        let span: f32 = g.cols.iter().skip(at).take(h.span).map(col_w).sum();
        let sep = g.cols.get(at).is_some_and(|c| c.sep);
        at += h.span;
        groups = groups.child(
            div()
                .w(px(span))
                .flex_none()
                .pt(px(12.))
                // A caption wider than its columns is cut there (Go: `text-overflow: ellipsis`), not over the next.
                .overflow_hidden()
                .pl(px(if sep { 10. } else { 4. }))
                .when(sep, |d| d.border_l_1().border_color(t.line2))
                .child(tracked(h.text, 11., 700., t.dim, 1.1)),
        );
    }
    let mut col_heads = div().flex().ml(px(left));
    for ci in cols {
        if let Some(c) = g.cols.get(ci) {
            col_heads = col_heads.child(parts::col_head(t, v, ci, c));
        }
    }
    div()
        .flex()
        .flex_none()
        .border_b_1()
        .border_color(t.line)
        .child(div().w(px(HEAD_W)).flex_none().child(parts::corner(t)))
        .child(
            div().flex_1().min_w_0().overflow_hidden().child(
                div()
                    .flex()
                    .flex_col()
                    .ml(px(-v.grid.x))
                    .child(groups)
                    .child(col_heads),
            ),
        )
}

/// The rows in view (and "needed by" once it is): their heads for the sticky column, their cells for the rest.
fn body(
    t: &Tokens,
    v: &OverviewView,
    rows: Range<usize>,
    cols: Range<usize>,
    cx: &mut Context<OverviewView>,
) -> (Vec<AnyElement>, Vec<AnyElement>) {
    let g = &v.home.grid;
    let shown = shown_rows(v);
    let (mut names, mut lines) = (Vec::new(), Vec::new());
    for ri in rows {
        let mut line = div().flex().h(px(ROW_H)).border_b_1().border_color(t.line);
        let Some(row) = shown.get(ri) else {
            names.push(parts::foot_head(t));
            for ci in cols.clone() {
                if let (Some(c), Some(f)) = (g.cols.get(ci), g.foot.get(ci)) {
                    line = line.child(parts::foot_cell(t, v, ci, c, f));
                }
            }
            lines.push(line.into_any_element());
            continue;
        };
        names.push(parts::row_head(t, v, ri, row));
        line = line.when(v.cur.is_some_and(|(cr, _)| cr == ri), |d| d.bg(t.sel));
        for ci in cols.clone() {
            if let (Some(c), Some(cell)) = (g.cols.get(ci), row.cells.get(ci)) {
                line = line.child(parts::cell(t, v, (ri, ci), (c, cell), cx));
            }
        }
        lines.push(line.into_any_element());
    }
    (names, lines)
}

/// The notes under the table: that there is nothing to cross yet, and what the Grid cannot show yet. They sit under
/// the scrolled part, whole, so the table never cuts them.
fn notes(t: &Tokens, g: &GridView) -> Vec<AnyElement> {
    let note = |text: &str| {
        div()
            .flex_none()
            .px(px(16.))
            .pt(px(10.))
            .pb(px(10.))
            .border_t_1()
            .border_color(t.line)
            .text_size(px(12.))
            .text_color(t.dim)
            .child(s(text))
            .into_any_element()
    };
    let mut out = Vec::new();
    if g.rows.is_empty() {
        out.push(note("No app here refers to another app or to a shared service yet, so there is nothing to cross. The Grid fills in as doze.hcl's apps refer to what they use."));
    }
    if !g.later.is_empty() {
        out.push(note(&g.later));
    }
    out
}
