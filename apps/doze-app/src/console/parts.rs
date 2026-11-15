//! A console's tables, counters, details and streams. A table of more rows than fit is a virtual list (only what
//! shows is laid out), so a module's thousand-row table costs what its forty visible rows cost; a stream keeps its
//! newest line in view (see `ConsoleTabView::refresh`).

use doze_desk::page::console::{Node, Row};
use gpui::{AnyElement, Div, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::blocks::{Draw, share};
use crate::instance::ui::{self, card, heading, text};
use crate::theme::ty;
use crate::widgets::{MONO_FAMILY, tracked};

/// A table's row height.
const ROW_H: f32 = 34.;
/// A table of more rows than this is drawn as a virtual list, this many rows tall, scrolling inside its frame.
pub(crate) const BIG: usize = 40;
/// How many rows a virtual table shows at once.
const SHOWN: usize = 16;

impl Draw<'_> {
    pub(super) fn widths(n: &Node) -> Vec<Option<f32>> {
        let last = n.cols.len().saturating_sub(1);
        n.cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (i < last).then(|| {
                    let widest = n
                        .rows
                        .iter()
                        .filter_map(|r| r.cells.get(i))
                        .map(|c| c.text.chars().count())
                        .chain([c.title.chars().count() + 2, 5])
                        .max()
                        .unwrap_or(5);
                    (widest as f32 * 7.4).min(280.)
                })
            })
            .collect()
    }

    pub(super) fn cells(
        &self,
        n: &Node,
        widths: &[Option<f32>],
        cells: Vec<(SharedString, gpui::Hsla)>,
    ) -> Div {
        let mut d = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .whitespace_nowrap();
        for ((i, (s, color)), width) in cells.into_iter().enumerate().zip(widths) {
            let right = n.cols.get(i).is_some_and(|c| c.right);
            let cell = match width {
                Some(wd) => div().w(px(*wd)).flex_none(),
                None => div().flex_1().min_w_0(),
            };
            d = d.child(
                cell.overflow_hidden()
                    .text_ellipsis()
                    .text_color(color)
                    .when(right, |c| c.text_right())
                    .child(s),
            );
        }
        d
    }

    fn header(&self, n: &Node, widths: &[Option<f32>]) -> Div {
        let t = self.t;
        let cells = n
            .cols
            .iter()
            .map(|c| (SharedString::from(c.title.to_uppercase()), t.dim))
            .collect();
        self.cells(n, widths, cells)
            .min_h(px(28.))
            .bg(t.well)
            .text_size(px(ty::XS))
            .font_weight(ty::SEMIBOLD)
    }

    fn row_cells(&self, r: &Row) -> Vec<(SharedString, gpui::Hsla)> {
        r.cells
            .iter()
            .map(|c| (SharedString::from(c.text.clone()), ui::ink(self.t, c.ink)))
            .collect()
    }

    fn table_row(
        &self,
        n: &Node,
        widths: &[Option<f32>],
        i: usize,
        r: &Row,
    ) -> gpui::Stateful<Div> {
        let t = self.t;
        let on = self.live.is_some() && i == n.selected;
        let mut row = self
            .cells(n, widths, self.row_cells(r))
            .id(SharedString::from(format!("{}-{i}", n.id)))
            .min_h(px(ROW_H))
            .border_t_1()
            .border_color(t.line)
            .when(r.faded, |d| d.opacity(0.55))
            .when(on, |d| d.bg(t.pick).text_color(t.bright))
            .when(!on && self.live.is_some(), |d| d.hover(|s| s.bg(t.sel)));
        if let Some(e) = &self.live {
            let (e, id) = (e.clone(), n.id.clone());
            row = row
                .cursor_pointer()
                .on_click(move |_, _, cx| e.update(cx, |v, cx| v.pick(&id, i, cx)));
        }
        row
    }

    pub(super) fn table(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let widths = Self::widths(n);
        let focused = self.focused.as_deref() == Some(n.id.as_str());
        let body: AnyElement = if n.rows.is_empty() {
            div()
                .px(px(12.))
                .min_h(px(ROW_H))
                .flex()
                .items_center()
                .border_t_1()
                .border_color(t.line)
                .text_color(t.dim)
                .child("nothing yet")
                .into_any_element()
        } else if n.rows.len() <= BIG {
            div()
                .flex()
                .flex_col()
                .children(
                    n.rows
                        .iter()
                        .enumerate()
                        .map(|(i, r)| self.table_row(n, &widths, i, r)),
                )
                .into_any_element()
        } else {
            self.big_table(n, widths)
        };
        share(div(), n)
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(if focused { t.line2 } else { t.line })
            .bg(t.panel)
            .overflow_hidden()
            .child(self.header(n, &Self::widths(n)))
            .child(body)
            .into_any_element()
    }

    /// A table too long to lay out whole: only the rows in view are built, the list scrolls in its frame, and the
    /// picked row is kept in view as the arrows move it (`ConsoleTabView::lists`).
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
                };
                range
                    .filter_map(|i| node.rows.get(i).map(|r| d.table_row(&node, &widths, i, r)))
                    .collect::<Vec<_>>()
            },
        )
        .w_full()
        .h(px(ROW_H * SHOWN as f32));
        let list = match self.lists.and_then(|l| l.get(&n.id)) {
            Some(h) => list.track_scroll(h).into_any_element(),
            None => list.into_any_element(),
        };
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(list)
            .child(
                div()
                    .px(px(12.))
                    .min_h(px(28.))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(t.line)
                    .text_size(px(ty::XS_UP))
                    .text_color(t.dim)
                    .child(text(&format!(
                        "{} rows · scroll, or ↑ ↓ to move",
                        doze_display::thousands(i64::try_from(n.rows.len()).unwrap_or(i64::MAX))
                    ))),
            )
            .into_any_element()
    }

    pub(super) fn stats(&self, n: &Node) -> AnyElement {
        let t = self.t;
        share(div(), n)
            .flex()
            .flex_wrap()
            .gap(px(10.))
            .children(n.stats.iter().map(|s| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .min_w(px(120.))
                    .flex_1()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(t.line)
                    .bg(t.panel)
                    .child(tracked(&s.label, ty::XS, ty::REGULAR, t.dim, 0.66))
                    .child(
                        div()
                            .text_size(px(ty::FIGURE))
                            .font_weight(ty::SEMIBOLD)
                            .text_color(match s.ink {
                                doze_desk::page::Ink::Text => t.bright,
                                i => ui::ink(t, i),
                            })
                            .child(text(&s.value)),
                    )
            }))
            .into_any_element()
    }

    pub(super) fn detail(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let mut c = share(card(t), n).child(heading(t, &n.title, None));
        match n.detail.split_last() {
            None => c = c.child(div().text_color(t.dim).child("nothing selected")),
            Some(((_, last), rest)) => {
                let facts: Vec<String> = rest
                    .iter()
                    .map(|(k, v)| {
                        if k.is_empty() {
                            v.clone()
                        } else {
                            format!("{} {v}", k.to_lowercase())
                        }
                    })
                    .collect();
                c = c
                    .child(
                        div()
                            .font_family(MONO_FAMILY)
                            .text_size(px(ty::SM))
                            .line_height(px(ty::LH_19))
                            .text_color(t.bright)
                            .child(text(last)),
                    )
                    .child(
                        div()
                            .text_size(px(ty::SM))
                            .text_color(t.dim)
                            .child(text(&facts.join(" · "))),
                    );
            }
        }
        c.into_any_element()
    }

    pub(super) fn stream(&self, n: &Node) -> AnyElement {
        let t = *self.t;
        let widths = Self::widths(n);
        let title = div()
            .px(px(12.))
            .min_h(px(28.))
            .flex()
            .items_center()
            .bg(t.well)
            .child(tracked(&n.title, ty::XS, ty::SEMIBOLD, t.dim, 0.55));
        let body: AnyElement = if n.rows.is_empty() {
            div()
                .px(px(12.))
                .min_h(px(28.))
                .flex()
                .items_center()
                .border_t_1()
                .border_color(t.line)
                .text_color(t.dim)
                .child("waiting for the first line")
                .into_any_element()
        } else {
            let (node, rows) = (n.clone(), n.rows.clone());
            let list = uniform_list(
                SharedString::from(format!("stream-{}", n.id)),
                rows.len(),
                move |range, _, _| {
                    let d = Draw {
                        t: &t,
                        focused: None,
                        live: None,
                        stream: None,
                        lists: None,
                    };
                    range
                        .filter_map(|i| rows.get(i))
                        .map(|r| {
                            d.cells(&node, &widths, d.row_cells(r))
                                .min_h(px(28.))
                                .border_t_1()
                                .border_color(t.line)
                                .font_family(MONO_FAMILY)
                                .text_size(px(ty::SM))
                        })
                        .collect::<Vec<_>>()
                },
            )
            .flex_1()
            .w_full()
            .min_h(px(120.));
            match &self.stream {
                Some(h) => list.track_scroll(h).into_any_element(),
                None => list.into_any_element(),
            }
        };
        share(div(), n)
            .flex()
            .flex_col()
            .min_h(px(160.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.line)
            .bg(t.panel)
            .overflow_hidden()
            .child(title)
            .child(self.header(n, &Self::widths(n)))
            .child(body)
            .into_any_element()
    }
}
