//! A console's tables (counters and details are in `figures`, streams in `stream`). A table of more rows than fit is a
//! virtual list (only what shows is laid out), so a module's thousand-row table costs what its forty visible rows cost.
//! Columns are as wide as what they hold, and the widest column of long words takes what is left (`Col::grow`).

use doze_desk::page::console::{FILTER_FROM, Node, Row};
use doze_display::NodeKind;
use gpui::{AnyElement, Div, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::blocks::{Draw, share};
use crate::instance::ui::{self, heading, text};
use crate::theme::ty;
use crate::widgets::kbd_chip;

/// A table's row height.
const ROW_H: f32 = 34.;
/// A stream's row height.
pub(super) const LINE_H: f32 = 28.;
/// A table of more rows than this is drawn as a virtual list, scrolling inside its card.
pub(crate) const BIG: usize = 40;
/// How many rows a virtual table shows at the least, when the page is short.
const SHOWN: usize = 8;
/// A character's width in the table face, and in the stream's mono face, at their sizes.
const CH: f32 = 7.6;
pub(super) const MONO_CH: f32 = 7.9;
/// The space between columns, and inside a row's ends.
const GAP: f32 = 16.;

/// Each column's width: a fixed one for what it holds, or `None` for the column that takes what is left.
pub(super) fn widths(n: &Node, ch: f32) -> Vec<Option<f32>> {
    n.cols
        .iter()
        .map(|c| (!c.grow).then_some(c.chars as f32 * ch))
        .collect()
}

impl Draw<'_> {
    pub(super) fn cells(&self, n: &Node, widths: &[Option<f32>], cells: Vec<AnyElement>) -> Div {
        let mut d = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(GAP))
            .px(px(14.))
            .whitespace_nowrap();
        for ((i, s), width) in cells.into_iter().enumerate().zip(widths) {
            let col = n.cols.get(i);
            let right = col.is_some_and(|c| c.right);
            let cell = match width {
                Some(wd) => div().w(px(*wd)).flex_none(),
                // The growing column keeps room for what it holds before the others; past that it is cut short.
                None => div()
                    .flex_1()
                    .min_w(px(col.map_or(6, |c| c.chars.min(18)) as f32 * CH)),
            };
            d = d.child(
                cell.overflow_hidden()
                    .text_ellipsis()
                    .flex()
                    .when(right, |c| c.justify_end())
                    .child(s),
            );
        }
        // No column grows: the room left is after the last, not between the columns.
        if widths.iter().all(Option::is_some) {
            d = d.child(div().flex_1());
        }
        d
    }

    /// The column titles: on a tab, a click orders the table by one (again, the other way; a third time, as sent).
    pub(super) fn header(&self, n: &Node, widths: &[Option<f32>], sortable: bool) -> Div {
        let t = self.t;
        let cells = n
            .cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mark = match c.sort {
                    Some(false) => " ↑",
                    Some(true) => " ↓",
                    None => "",
                };
                let mut title = div()
                    .id(SharedString::from(format!("{}-h{i}", n.id)))
                    .flex()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(if c.sort.is_some() { t.text } else { t.dim })
                    .child(SharedString::from(format!(
                        "{}{mark}",
                        c.title.to_uppercase()
                    )));
                if let (true, Some(e)) = (sortable, &self.live) {
                    let (e, id) = (e.clone(), n.id.clone());
                    title = title
                        .cursor_pointer()
                        .hover(|s| s.text_color(t.text))
                        .on_click(move |_, _, cx| e.update(cx, |v, cx| v.sort(&id, i, cx)));
                }
                title.into_any_element()
            })
            .collect();
        self.cells(n, widths, cells)
            .min_h(px(30.))
            .bg(t.well)
            .text_size(px(ty::XS))
            .font_weight(ty::SEMIBOLD)
    }

    pub(super) fn row_cells(&self, r: &Row) -> Vec<AnyElement> {
        r.cells
            .iter()
            .map(|c| {
                div()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(ui::ink(self.t, c.ink))
                    .child(SharedString::from(c.text.clone()))
                    .into_any_element()
            })
            .collect()
    }

    fn table_row(&self, n: &Node, widths: &[Option<f32>], r: &Row) -> gpui::Stateful<Div> {
        let t = self.t;
        let on = self.live.is_some() && r.at == n.selected;
        let focused = self.focused.as_deref() == Some(n.id.as_str());
        let mut row = self
            .cells(n, widths, self.row_cells(r))
            .id(SharedString::from(format!("{}-{}", n.id, r.at)))
            .relative()
            .h(px(ROW_H))
            .flex_none()
            .border_t_1()
            .border_color(t.line)
            .when(r.faded, |d| d.opacity(0.55))
            .when(on, |d| {
                d.bg(if focused { t.pick } else { t.sel })
                    .text_color(t.bright)
                    .child(crate::widgets::sel_bar(t, 0.).when(!focused, |b| b.opacity(0.)))
            })
            .when(!on && self.live.is_some(), |d| d.hover(|s| s.bg(t.sel)));
        // The selected row carries what can be done to it, unless a detail beside the table shows that.
        if on && !n.detailed && !n.row_actions.is_empty() {
            row = row.child(self.row_actions(n));
        }
        if let Some(e) = &self.live {
            let (e, id, at) = (e.clone(), n.id.clone(), r.at);
            row = row
                .cursor_pointer()
                .on_click(move |_, _, cx| e.update(cx, |v, cx| v.pick(&id, at, cx)));
        }
        row
    }

    /// The selected row's actions, over its right end: each its letter and its label, a click runs it.
    fn row_actions(&self, n: &Node) -> Div {
        let t = self.t;
        let mut d = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(12.))
            .bg(t.pick);
        for (i, a) in n.row_actions.iter().enumerate() {
            let mut b = div()
                .id(SharedString::from(format!("{}-ra{i}", n.id)))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.line2)
                .bg(t.panel)
                .text_size(px(ty::SM))
                .text_color(if a.destructive { t.bad } else { t.text })
                .child(kbd_chip(t, &a.key.to_string(), ty::XS, ty::SEMIBOLD, 5.))
                .child(text(&a.label));
            if let Some(e) = &self.live {
                let (e, a) = (e.clone(), a.clone());
                b = b
                    .cursor_pointer()
                    .hover(|s| s.bg(t.sel))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        e.update(cx, |v, cx| v.act(a.clone(), cx));
                    });
            }
            d = d.child(b);
        }
        d
    }

    pub(super) fn table(&self, n: &Node, dir: Option<&NodeKind>) -> AnyElement {
        let t = self.t;
        let widths = widths(n, CH);
        let focused = self.focused.as_deref() == Some(n.id.as_str());
        let big = n.total > BIG;
        let body: AnyElement = if n.rows.is_empty() {
            div()
                .px(px(14.))
                .h(px(ROW_H))
                .flex()
                .items_center()
                .border_t_1()
                .border_color(t.line)
                .text_color(t.dim)
                .child(if n.total > 0 {
                    "no row holds those words"
                } else {
                    "nothing yet"
                })
                .into_any_element()
        } else if !big {
            div()
                .flex()
                .flex_col()
                .children(n.rows.iter().map(|r| self.table_row(n, &widths, r)))
                .into_any_element()
        } else {
            self.big_table(n, widths.clone())
        };
        // A short table is as tall as its rows; a long one takes the room left and scrolls inside.
        let frame = if big || dir != Some(&NodeKind::Col) {
            share(div(), n)
        } else {
            div().flex_none().min_w_0()
        };
        let frame = if self.flat {
            frame.flex().flex_col().overflow_hidden()
        } else {
            frame
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(if focused && self.live.is_some() {
                    t.line2
                } else {
                    t.line
                })
                .bg(t.panel)
                .shadow(t.lift())
                .overflow_hidden()
        };
        frame
            .children(self.flat_heading(n))
            .child(self.header(n, &widths, self.live.is_some()))
            .child(body)
            .children(self.foot(n))
            .into_any_element()
    }

    /// Under a long table: how many rows it has (and how many the filter shows), and the filter box or its key.
    fn foot(&self, n: &Node) -> Option<Div> {
        let t = self.t;
        let open = self.filter.as_ref().filter(|(id, _, _)| *id == n.id);
        if n.total <= FILTER_FROM && open.is_none() && n.filter.is_empty() {
            return None;
        }
        let count = doze_display::thousands(i64::try_from(n.total).unwrap_or(i64::MAX));
        let said = if n.filter.trim().is_empty() {
            format!("{count} rows")
        } else {
            let shown = doze_display::thousands(i64::try_from(n.rows.len()).unwrap_or(i64::MAX));
            format!("{shown} of {count} rows")
        };
        let mut d = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .h(px(38.))
            .border_t_1()
            .border_color(t.line)
            .text_size(px(ty::SM))
            .text_color(t.dim);
        match open {
            Some((_, input, keys)) => {
                d = d.child(
                    crate::widgets::framed_input(
                        t,
                        &format!("console/{}/filter", n.id),
                        *keys,
                        gpui_component::input::Input::new(input),
                    )
                    .w(px(240.))
                    .flex_none(),
                );
            }
            None if self.live.is_some() => {
                d = d.child(kbd_chip(t, "/", ty::XS, ty::SEMIBOLD, 5.)).child(
                    if n.filter.is_empty() {
                        text("filter")
                    } else {
                        text(&format!("filtered to “{}”", n.filter))
                    },
                );
            }
            None => {}
        }
        Some(
            d.child(div().flex_1())
                .child(
                    div()
                        .font_features(ty::tabular())
                        .child(text(&if self.live.is_some() && n.total > BIG {
                            format!("{said} · scroll, or ↑ ↓")
                        } else {
                            said
                        })),
                ),
        )
    }

    /// A table too long to lay out whole: only the rows in view are built, the list scrolls in its card and takes the
    /// height the page has left, and the picked row is kept in view as the arrows move it
    /// (`ConsoleTabView::lists`).
    fn big_table(&self, n: &Node, widths: Vec<Option<f32>>) -> AnyElement {
        let t = *self.t;
        let (node, live, focused) = (n.clone(), self.live.clone(), self.focused.clone());
        let list = uniform_list(
            SharedString::from(format!("table-{}", n.id)),
            n.rows.len(),
            move |range, _, _| {
                let d = Draw {
                    t: &t,
                    focused: focused.clone(),
                    live: live.clone(),
                    stream: None,
                    lists: None,
                    filter: None,
                    flat: false,
                };
                range
                    .filter_map(|i| node.rows.get(i).map(|r| d.table_row(&node, &widths, r)))
                    .collect::<Vec<_>>()
            },
        )
        .w_full()
        .flex_1()
        .min_h(px(ROW_H * SHOWN as f32));
        let list = match self.lists.and_then(|l| l.get(&n.id)) {
            Some(h) => list.track_scroll(h).into_any_element(),
            None => list.into_any_element(),
        };
        div()
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h(px(ROW_H * SHOWN as f32))
            .flex()
            .flex_col()
            .child(list)
            .into_any_element()
    }

    /// On the overview a table or the figures have no card of their own: the block's title is said above it.
    pub(super) fn flat_heading(&self, n: &Node) -> Option<Div> {
        (self.flat && !n.title.is_empty()).then(|| {
            div()
                .px(px(14.))
                .pt(px(12.))
                .pb(px(4.))
                .child(heading(self.t, &n.title, None))
        })
    }
}
