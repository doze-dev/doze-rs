//! The Grid's pieces: a column's head, a row's head (the service on the left), a cell with its dot and the tooltip
//! under it, and the "needed by" counts.

use doze_desk::overview::{GridCell, GridCol, GridRow, GridTip};
use gpui::{AnyElement, Context, IntoElement, anchored, deferred, div, point, prelude::*, px};

use super::{ROW_H, col_w};
use crate::kit::{s, tone};
use crate::overlays::shadow;
use crate::overview::OverviewView;
use crate::overview::tiles::row_dot;
use crate::theme::{Tokens, mix, ty};
use crate::widgets::{Mono as _, tracked};

/// A cell's box: its width, its line on the left when it starts a group, the column's light.
pub(super) fn cell_box(t: &Tokens, v: &OverviewView, ci: usize, c: &GridCol) -> gpui::Div {
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

/// The service column's caption: `SERVICE needs →`.
pub(super) fn corner(t: &Tokens) -> impl IntoElement {
    div()
        .pt(px(12.))
        .pl(px(16.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(tracked("Service", ty::XS, ty::BOLD, t.dim, 1.1))
        .child(
            div()
                .text_size(px(ty::XS))
                .text_color(t.dim)
                .child("needs →"),
        )
}

/// A column's head: its dot, name and memory; the tasks' tally.
pub(super) fn col_head(t: &Tokens, v: &OverviewView, ci: usize, c: &GridCol) -> AnyElement {
    let inner = if c.kind == "tasks" {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(3.))
            // Where the other columns have their dot, so the names line up.
            .child(div().size(px(8.)))
            .child(
                div()
                    .font_weight(ty::MEDIUM)
                    .text_color(t.dim)
                    .child("tasks"),
            )
            .child(
                div()
                    .text_size(px(ty::XS))
                    .text_color(t.dim)
                    .child(s(&c.sub)),
            )
    } else {
        let r = crate::overview::all_rows(&v.home)
            .into_iter()
            .find(|r| r.name == c.name);
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
                    .font_weight(ty::SEMIBOLD)
                    .text_color(t.bright)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(s(&c.name)),
            )
            .child(
                div()
                    .text_size(px(ty::XS))
                    .line_height(px(ty::LH_14))
                    .text_color(t.dim)
                    .font_features(ty::tabular())
                    .child(s(
                        r.map_or(" ", |r| if r.mem.is_empty() { " " } else { &r.mem })
                    )),
            )
    };
    cell_box(t, v, ci, c)
        .pt(px(10.))
        .pb(px(8.))
        .px(px(4.))
        .items_end()
        .text_size(px(ty::SM_UP))
        .child(inner)
        .into_any_element()
}

/// A row's head: the service's dot, name, language and memory; lit with its row.
pub(super) fn row_head(t: &Tokens, v: &OverviewView, ri: usize, row: &GridRow) -> AnyElement {
    let r = crate::overview::all_rows(&v.home)
        .into_iter()
        .find(|x| x.name == row.name);
    let mut head = div()
        .h(px(ROW_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(16.))
        .pr(px(18.))
        .border_b_1()
        .border_color(t.line)
        .when(v.cur.is_some_and(|(cr, _)| cr == ri), |d| d.bg(t.sel))
        .children(r.map(|r| row_dot(t, r)))
        .child(
            div()
                .min_w_0()
                .text_size(px(ty::BASE_UP))
                .font_weight(ty::SEMIBOLD)
                .text_color(t.bright)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&row.name)),
        );
    if !row.lang.is_empty() {
        head = head.child(
            div()
                .flex_none()
                .px(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color(t.line2)
                .mono_face()
                .text_size(px(ty::MICRO))
                .line_height(px(ty::LH_13))
                .text_color(t.dim)
                .child(row.lang),
        );
    }
    if let Some(m) = r.map(|r| r.mem.as_str()).filter(|m| !m.is_empty()) {
        head = head.child(div().flex_1()).child(
            div()
                .flex_none()
                .text_size(px(ty::SM_UP))
                .font_weight(ty::SEMIBOLD)
                .text_color(t.dim)
                .font_features(ty::tabular())
                .child(s(m)),
        );
    }
    head.into_any_element()
}

/// One cell: a dot where the row needs the column, a dash on its own column, a task's word; the tooltip under it
/// while the crosshair is on it, drawn deferred and anchored so the table's box never clips it.
pub(super) fn cell(
    t: &Tokens,
    v: &OverviewView,
    (ri, ci): (usize, usize),
    (c, cell): (&GridCol, &GridCell),
    cx: &mut Context<OverviewView>,
) -> AnyElement {
    let cross = v.cur == Some((ri, ci));
    let mut el = cell_box(t, v, ci, c)
        .id(("grid-cell", ri * 100_000 + ci))
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
                .text_size(px(ty::SM))
                .text_color(tone(t, cell.tone))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&cell.text)),
        );
    }
    if let (true, Some(tip)) = (cross, &cell.tip) {
        // Anchored at the cell's top left (a centred flex child would centre the tip on the cell instead), its arrow on
        // the cell's middle: under the cell, or above it when it would run past the bottom of the table (Go: `top =
        // t.top - h - 8`).
        let top = tip_top(ri, v.grid.y, v.grid.port.get().1);
        let tip = deferred(
            anchored()
                .offset(point(px(col_w(c) / 2. - 28.), px(top)))
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
        .text_size(px(ty::SM))
        .line_height(px(ty::LH_18))
        .shadow(shadow(t))
        .whitespace_nowrap()
        .child(arrow)
        .child(
            div()
                .flex()
                .child(div().font_weight(ty::SEMIBOLD).child(s(&tip.head)))
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
                        .mono_face()
                        .text_color(mix(t.accent, t.bg, 45.))
                        .child(s(&tip.reference)),
                )
                .child(div().opacity(0.7).child("in doze.hcl")),
        )
        .child(div().opacity(0.75).child(s(&tip.keys)))
}

/// "needed by" under the service column.
pub(super) fn foot_head(t: &Tokens) -> AnyElement {
    div()
        .h(px(ROW_H))
        .flex_none()
        .flex()
        .items_center()
        .pl(px(16.))
        .border_b_1()
        .border_color(t.line)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .child("needed by")
        .into_any_element()
}

/// A column's "needed by" count.
pub(super) fn foot_cell(
    t: &Tokens,
    v: &OverviewView,
    ci: usize,
    c: &GridCol,
    f: &str,
) -> AnyElement {
    cell_box(t, v, ci, c)
        .h_full()
        .text_size(px(ty::SM))
        .child(
            div()
                .font_weight(ty::SEMIBOLD)
                .text_color(t.text)
                .child(s(f)),
        )
        .into_any_element()
}

/// The tooltip's height as Go guesses it before it is measured (`offsetHeight ?? 70`); three lines of 18 and their
/// padding come to about this.
const TIP_H: f32 = 74.;

/// Where the tooltip's top goes, from the top of row `ri`'s cell: 8 px under the cell, or, when that would run past
/// the bottom of the part that scrolls (`port_h` tall, scrolled `scroll_y` down), 8 px over it.
pub(super) fn tip_top(ri: usize, scroll_y: f32, port_h: f32) -> f32 {
    #[allow(clippy::cast_precision_loss)] // WHY: a row index, far under 2^24
    let cell_top = ri as f32 * ROW_H - scroll_y;
    let under = ROW_H + 8.;
    if port_h > 0. && cell_top + under + TIP_H > port_h - 4. {
        -(TIP_H + 8.)
    } else {
        under
    }
}

#[cfg(test)]
mod tests {
    use super::{ROW_H, TIP_H, tip_top};

    /// Go's tooltip sits under its cell and flips above it near the bottom of the box.
    #[test]
    fn the_tip_flips_above_near_the_bottom() {
        assert_eq!(tip_top(0, 0., 400.), ROW_H + 8.);
        assert_eq!(tip_top(8, 0., 400.), -(TIP_H + 8.));
        // Scrolled so row 8 is near the top again: under it.
        assert_eq!(tip_top(8, 8. * ROW_H, 400.), ROW_H + 8.);
        // Not measured yet: under.
        assert_eq!(tip_top(8, 0., 0.), ROW_H + 8.);
    }
}
