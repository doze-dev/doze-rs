//! A console's tables (counters and details are in `figures`, streams in `stream`). A table of more rows than fit is a
//! virtual list (only what shows is laid out), so a module's thousand-row table costs what its forty visible rows cost.
//! Columns are as wide as what they hold; the widest column of long words takes what is left (`Col::grow`), and with
//! none, the room left is shared out by how much each holds. What can be done to the selected row has a column of its
//! own at the end, never over the cells.

use doze_desk::page::console::{FILTER_FROM, Node, Row};
use doze_display::NodeKind;
use gpui::{AnyElement, Div, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::blocks::{Draw, share};
use crate::instance::ui::{self, heading, text};
use crate::theme::ty;
use crate::widgets::kbd_chip;

/// A table's row height.
pub(super) const ROW_H: f32 = 34.;
/// A stream's row height.
pub(super) const LINE_H: f32 = 28.;
/// A table of more rows than this is drawn as a list that takes the height left on the page and scrolls inside its
/// card, its footer ("16 rows · scroll") always above the keys bar: a page of rows running under the bar hid both.
pub(crate) const BIG: usize = doze_desk::page::console::FILTER_FROM;
/// How many rows a virtual table shows at the least, when the page is short.
const SHOWN: usize = 4;
/// A character's width in the table face, and in the stream's mono face, at their sizes.
pub(super) const CH: f32 = 7.6;
pub(super) const MONO_CH: f32 = 7.9;
/// The space between columns, and inside a row's ends.
pub(super) const GAP: f32 = 16.;

/// Each column's width: a fixed one for what it holds, or `None` for the column that takes what is left.
pub(super) fn widths(n: &Node, ch: f32) -> Vec<Option<f32>> {
    n.cols
        .iter()
        .map(|c| (!c.grow).then_some(c.chars as f32 * ch))
        .collect()
}

impl Draw<'_> {
    pub(super) fn cells(
        &self,
        n: &Node,
        widths: &[Option<f32>],
        cells: Vec<AnyElement>,
        tail: Option<(f32, AnyElement)>,
    ) -> Div {
        let grows = widths.iter().any(Option::is_none);
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
                // A column of long words takes the room left; with none, each takes a share of it by what it holds.
                // A column is never narrower than what it holds: the actions column shows letters only first.
                Some(wd) if grows => div().w(px(*wd)).flex_none(),
                // With none, the room left goes to the columns of words, by what they hold: a figure keeps its width, so it
                // stays beside the words before it rather than across a gap at the far edge.
                Some(wd) => div()
                    .flex_basis(px(*wd))
                    .flex_grow(if right { 0. } else { wd.max(1.) })
                    .flex_shrink_0()
                    .min_w(px(*wd)),
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
        if let Some((w, el)) = tail {
            d = d.child(div().w(px(w)).flex_none().flex().justify_end().child(el));
        }
        d
    }

    /// A block's title over its rows: a heading in the card (on the overview, the same heading, flat).
    pub(super) fn title_bar(&self, n: &Node) -> Option<Div> {
        if self.flat {
            return self.flat_heading(n);
        }
        (!n.title.is_empty()).then(|| {
            div()
                .px(px(16.))
                .pt(px(4.))
                .min_h(px(34.))
                .flex()
                .items_center()
                .child(heading(self.t, &n.title, None))
        })
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
                // A title cut short ends in an ellipsis, as a cell does, never mid-letter.
                let mut title = div()
                    .id(SharedString::from(format!("{}-h{i}", n.id)))
                    .min_w_0()
                    .whitespace_nowrap()
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
        let tail = self.gutter(n).map(|w| (w, div().into_any_element()));
        self.cells(n, widths, cells, tail)
            .min_h(px(30.))
            .bg(t.well)
            .text_size(px(ty::XS))
            .font_weight(ty::SEMIBOLD)
    }

    pub(super) fn row_cells(&self, r: &Row) -> Vec<AnyElement> {
        let t = self.t;
        r.cells
            .iter()
            .map(|c| {
                // A state with no glyph of its own is written with a dot before it ("· idle"); the dim ink says it.
                let text = c.text.strip_prefix("· ").unwrap_or(&c.text);
                let ink = match c.ink {
                    // The amber is a little light on a white card: a step towards the ink there.
                    doze_desk::page::Ink::Warn if !t.dark => {
                        crate::theme::mix(t.warn, t.bright, 30.)
                    }
                    i => ui::ink(t, i),
                };
                div()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(ink)
                    .child(SharedString::from(text.to_owned()))
                    .into_any_element()
            })
            .collect()
    }

    fn table_row(&self, n: &Node, widths: &[Option<f32>], r: &Row) -> gpui::Stateful<Div> {
        let t = self.t;
        let on = self.live.is_some() && r.at == n.selected;
        let focused = self.focused.as_deref() == Some(n.id.as_str());
        // The selected row's actions sit in the table's actions column, never over its cells.
        let tail = self.gutter(n).map(|w| {
            let el = if on {
                self.row_actions(n).into_any_element()
            } else {
                div().into_any_element()
            };
            (w, el)
        });
        let mut row = self
            .cells(n, widths, self.row_cells(r), tail)
            .id(SharedString::from(format!("{}-{}", n.id, r.at)))
            .relative()
            .h(px(ROW_H))
            .flex_none()
            .border_t_1()
            .border_color(t.line)
            .when(r.faded, |d| d.opacity(0.55))
            .when(on, |d| {
                // Asleep, nothing can act on it: the selection is muted.
                let lit = focused && !self.asleep;
                d.bg(if lit { t.pick } else { t.sel })
                    .text_color(t.bright)
                    .child(crate::widgets::sel_bar(t, 0.).when(!lit, |b| b.opacity(0.)))
            })
            .when(!on && self.live.is_some(), |d| d.hover(|s| s.bg(t.sel)));
        if let Some(e) = &self.live {
            let (e, id, at) = (e.clone(), n.id.clone(), r.at);
            row = row
                .cursor_pointer()
                .on_click(move |_, _, cx| e.update(cx, |v, cx| v.pick(&id, at, cx)));
        }
        row
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
            .relative()
            .children(self.measure_width(n))
            .children(self.title_bar(n))
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
        let (asleep, rooms) = (self.asleep, self.rooms.clone());
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
                    asleep,
                    // The same measurements as the header: its actions column is the rows' width.
                    rooms: rooms.clone(),
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
