//! The Grid view (the MapGrid board): the stack as a table that reads the same at thirty services as at five. A row
//! needs each column it has a dot in. Hovering a cell (or moving to it with the arrows) lights its row and its column,
//! and a tooltip under it says what the dot means, why it matters and the line of doze.hcl that makes it so.
//!
//! The tooltip is drawn deferred and anchored to its cell, so it is never clipped by the table's box and stays inside
//! the window.

use doze_desk::overview::{GridCell, GridCol, GridRow, GridTip, GridView};
use gpui::{AnyElement, Context, IntoElement, anchored, deferred, div, point, prelude::*, px};

use super::OverviewView;
use super::tiles::{filter, row_dot};
use crate::kit::{s, tone};
use crate::overlays::shadow;
use crate::theme::{Tokens, mix};
use crate::widgets::{MONO_FAMILY, tracked, w};

const HEAD_W: f32 = 300.;
const ROW_H: f32 = 42.;

fn col_w(c: &GridCol) -> f32 {
    match c.kind {
        "app" => 74.,
        "shared" => 68.,
        _ => 136.,
    }
}

/// Moves the crosshair with the arrows.
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
    if let Some(name) = shown_rows(v).get(r).map(|row| row.name.clone()) {
        v.select(&name, cx);
    }
    cx.notify();
}

fn shown_rows(v: &OverviewView) -> Vec<&GridRow> {
    let shown: Vec<&str> = v.shown().iter().map(|r| r.name.as_str()).collect();
    v.home
        .grid
        .rows
        .iter()
        .filter(|r| shown.contains(&r.name.as_str()))
        .collect()
}

/// The view: the filter (once asked for), the table, what it cannot show yet.
pub(super) fn view(
    t: &Tokens,
    v: &OverviewView,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let g = &v.home.grid;
    let rows = shown_rows(v);
    let mut table = div()
        .flex()
        .flex_col()
        .min_w_full()
        .child(heads(t, v))
        .child(col_heads(t, v));
    for (ri, row) in rows.iter().enumerate() {
        table = table.child(body_row(t, v, ri, row, cx));
    }
    table = table.child(foot(t, v));
    table = table.children(notes(t, g));
    let scroll = div()
        .id("grid-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_scroll()
        .child(table);
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
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered && this.cur.is_some() {
                this.cur = None;
                cx.notify();
            }
        }))
        .child(scroll);
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

/// A cell's box: its width, its line on the left when it starts a group, the column's light.
fn cell_box(t: &Tokens, v: &OverviewView, ci: usize, c: &GridCol) -> gpui::Div {
    let lit = v.cur.is_some_and(|(_, cc)| cc == ci);
    div()
        .w(px(col_w(c)))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .when(c.sep, |d| d.border_l_1().border_color(t.line2))
        .when(lit, |d| d.bg(mix(t.sel, t.panel, 55.)))
}

/// The groups' captions over the columns.
fn heads(t: &Tokens, v: &OverviewView) -> impl IntoElement {
    let g = &v.home.grid;
    // The padding is the cells', so a group's line on the left runs from the box's top.
    let mut row = div().flex().child(
        div()
            .w(px(HEAD_W))
            .pt(px(12.))
            .flex_none()
            .pl(px(16.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(tracked("Service", 11., 700., t.dim, 1.1))
            .child(div().text_size(px(11.)).text_color(t.dim).child("needs →")),
    );
    let mut at = 0;
    for h in &g.heads {
        let span: f32 = g.cols.iter().skip(at).take(h.span).map(col_w).sum();
        let sep = g.cols.get(at).is_some_and(|c| c.sep);
        at += h.span;
        row = row.child(
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
    row.child(div().flex_1())
}

/// Each column's head: its dot, name and memory; the tasks' tally.
fn col_heads(t: &Tokens, v: &OverviewView) -> impl IntoElement {
    let by_name = |n: &str| super::all_rows(&v.home).into_iter().find(|r| r.name == n);
    let mut row = div()
        .flex()
        .border_b_1()
        .border_color(t.line)
        .child(div().w(px(HEAD_W)).flex_none());
    for (ci, c) in v.home.grid.cols.iter().enumerate() {
        let inner = if c.kind == "tasks" {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(3.))
                // Where the other columns have their dot, so the names line up.
                .child(div().size(px(8.)))
                .child(div().font_weight(w(500.)).text_color(t.dim).child("tasks"))
                .child(div().text_size(px(11.)).text_color(t.dim).child(s(&c.sub)))
        } else {
            let r = by_name(&c.name);
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(3.))
                .min_w_0()
                .children(r.map(|r| row_dot(t, r)))
                .child(
                    div()
                        .max_w_full()
                        .font_weight(w(600.))
                        .text_color(t.bright)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(s(&c.name)),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(t.dim)
                        .child(s(
                            r.map_or(" ", |r| if r.mem.is_empty() { " " } else { &r.mem })
                        )),
                )
        };
        row = row.child(
            cell_box(t, v, ci, c)
                .pt(px(10.))
                .pb(px(8.))
                .px(px(4.))
                .items_end()
                .text_size(px(12.5))
                .child(inner),
        );
    }
    row.child(div().flex_1())
}

fn body_row(
    t: &Tokens,
    v: &OverviewView,
    ri: usize,
    row: &GridRow,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let r = super::all_rows(&v.home)
        .into_iter()
        .find(|x| x.name == row.name);
    let lit_row = v.cur.is_some_and(|(cr, _)| cr == ri);
    let mut head = div()
        .w(px(HEAD_W))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(16.))
        .pr(px(18.))
        .children(r.map(|r| row_dot(t, r)))
        .child(
            div()
                .text_size(px(13.5))
                .font_weight(w(600.))
                .text_color(t.bright)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&row.name)),
        );
    if !row.lang.is_empty() {
        head = head.child(
            div()
                .px(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color(t.line2)
                .font_family(MONO_FAMILY)
                .text_size(px(10.))
                .line_height(px(13.))
                .text_color(t.dim)
                .child(row.lang),
        );
    }
    if let Some(m) = r.map(|r| r.mem.as_str()).filter(|m| !m.is_empty()) {
        head = head.child(div().flex_1()).child(
            div()
                .text_size(px(12.5))
                .font_weight(w(600.))
                .text_color(t.dim)
                .child(s(m)),
        );
    }
    let mut line = div()
        .flex()
        .h(px(ROW_H))
        .border_b_1()
        .border_color(t.line)
        .when(lit_row, |d| d.bg(t.sel))
        .child(head);
    for (ci, (c, cell)) in v.home.grid.cols.iter().zip(&row.cells).enumerate() {
        line = line.child(cell_el(t, v, (ri, ci), (c, cell), cx));
    }
    line.child(div().flex_1())
}

fn cell_el(
    t: &Tokens,
    v: &OverviewView,
    (ri, ci): (usize, usize),
    (c, cell): (&GridCol, &GridCell),
    cx: &mut Context<OverviewView>,
) -> AnyElement {
    let cross = v.cur == Some((ri, ci));
    let mut el = cell_box(t, v, ci, c)
        .id(("grid-cell", ri * 10_000 + ci))
        .relative()
        .h_full()
        .px(px(6.))
        .when(cross && cell.dot, |d| d.bg(t.pick))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered && this.cur != Some((ri, ci)) {
                this.cur = Some((ri, ci));
                cx.notify();
            }
        }));
    if cell.dot {
        let dot = div().size(px(12.)).rounded_full().bg(t.accent);
        // Under the crosshair, a halo round the dot (`box-shadow: 0 0 0 4px`), the dot itself unchanged.
        let halo = div()
            .size(px(20.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .when(cross, |d| d.bg(crate::theme::alpha(t.accent, 0.3)))
            .child(dot);
        el = el.child(halo);
    } else if cell.own {
        el = el.child(div().text_color(t.line2).child("—"));
    } else if !cell.text.is_empty() {
        el = el.child(
            div()
                .text_size(px(12.))
                .text_color(tone(t, cell.tone))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&cell.text)),
        );
    }
    if let (true, Some(tip)) = (cross, &cell.tip) {
        // Anchored at the cell's top left (a centred flex child would centre the tip on the cell instead), its arrow on
        // the cell's middle.
        let tip = deferred(
            anchored()
                .offset(point(px(col_w(c) / 2. - 28.), px(ROW_H + 8.)))
                .snap_to_window_with_margin(px(8.))
                .child(tooltip(t, tip)),
        )
        .with_priority(1);
        el = el.child(div().absolute().top(px(0.)).left(px(0.)).child(tip));
    }
    el.into_any_element()
}

/// The tooltip: what the dot means and why · the line of doze.hcl · what the keys do.
fn tooltip(t: &Tokens, tip: &GridTip) -> impl IntoElement {
    let arrow = div().absolute().top(px(-6.)).left(px(22.)).child(
        gpui::svg()
            .path("icons/caret.svg")
            .w(px(12.))
            .h(px(6.))
            .text_color(t.bright),
    );
    div()
        .relative()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(9.))
        .bg(t.bright)
        .text_color(t.bg)
        .text_size(px(12.))
        .line_height(px(18.))
        .shadow(vec![shadow(t)])
        .whitespace_nowrap()
        .child(arrow)
        .child(
            div()
                .flex()
                .child(div().font_weight(w(600.)).child(s(&tip.head)))
                .when(!tip.why.is_empty(), |d| {
                    d.child(s(&format!(" · {}", tip.why)))
                }),
        )
        .child(
            div()
                .flex()
                .gap(px(4.))
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_color(mix(t.accent, t.bg, 45.))
                        .child(s(&tip.reference)),
                )
                .child(div().opacity(0.7).child("in doze.hcl")),
        )
        .child(div().opacity(0.75).child(s(&tip.keys)))
}

/// "needed by": a count under each column.
fn foot(t: &Tokens, v: &OverviewView) -> impl IntoElement {
    let g = &v.home.grid;
    let mut row = div()
        .flex()
        .h(px(38.))
        .border_b_1()
        .border_color(t.line)
        .items_center()
        .text_size(px(12.))
        .text_color(t.dim)
        .child(
            div()
                .w(px(HEAD_W))
                .flex_none()
                .pl(px(16.))
                .child("needed by"),
        );
    for (ci, (c, f)) in g.cols.iter().zip(&g.foot).enumerate() {
        row = row.child(
            cell_box(t, v, ci, c)
                .h_full()
                .child(div().font_weight(w(600.)).text_color(t.text).child(s(f))),
        );
    }
    row.child(div().flex_1())
}

/// The notes under the table: that there is nothing to cross yet, and what the Grid cannot show yet. They follow the
/// table, as Go's did, so the table is never cut by them.
fn notes(t: &Tokens, g: &GridView) -> Vec<gpui::AnyElement> {
    let note = |text: &str| {
        div()
            .px(px(16.))
            .pt(px(10.))
            .pb(px(8.))
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
