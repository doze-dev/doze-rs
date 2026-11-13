//! A console's node tree drawn: rows and columns share their room by weight; a table's rows pick, a stream keeps its
//! newest line in view, an action runs by its letter. The same drawing serves a module's blocks on the overview,
//! without the picking.

use doze_desk::page::console::{Node, Row};
use doze_display::NodeKind;
use gpui::{
    AnyElement, Context, Div, Entity, IntoElement, SharedString, div, prelude::*, px, relative,
    uniform_list,
};
use gpui_component::chart::AreaChart;

use super::ConsoleTabView;
use crate::instance::ui::{self, card, heading, text};
use crate::theme::{Tokens, alpha};
use crate::widgets::{MONO_FAMILY, kbd_chip, tracked, w};

/// What a drawing can do: pick rows and run actions on a console tab; nothing on the overview.
struct Draw<'a> {
    t: &'a Tokens,
    focused: Option<String>,
    live: Option<Entity<ConsoleTabView>>,
    stream: Option<gpui::UniformListScrollHandle>,
}

/// The tab's tree, live.
pub(crate) fn tab_body(
    t: &Tokens,
    v: &ConsoleTabView,
    cx: &mut Context<ConsoleTabView>,
) -> AnyElement {
    let d = Draw {
        t,
        focused: v.focused_table(),
        live: Some(cx.entity()),
        stream: Some(v.stream.clone()),
    };
    div()
        .id("con")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(14.))
        .px(px(16.))
        .py(px(14.))
        .overflow_y_scroll()
        .child(d.node(&v.page.body, None))
        .into_any_element()
}

/// A block on the overview: drawn, not picked.
pub(crate) fn static_block(t: &Tokens, n: &Node) -> AnyElement {
    let d = Draw {
        t,
        focused: None,
        live: None,
        stream: None,
    };
    div().p(px(12.)).child(d.node(n, None)).into_any_element()
}

/// Lets a block take its share of its parent's room, or only what it needs.
fn share(d: Div, n: &Node) -> Div {
    if n.flexible {
        d.flex_basis(relative(0.))
            .flex_grow(n.weight.max(1) as f32)
            .min_w_0()
            .min_h_0()
    } else {
        d.flex_none().min_w_0()
    }
}

impl Draw<'_> {
    fn node(&self, n: &Node, dir: Option<NodeKind>) -> AnyElement {
        let t = self.t;
        if !n.missing.is_empty() {
            return share(card(t), n)
                .text_color(t.dim)
                .child(text(&n.missing))
                .into_any_element();
        }
        match n.kind {
            NodeKind::Row | NodeKind::Col => {
                let row = n.kind == NodeKind::Row;
                let mut d = div().flex().gap(px(if row { 14. } else { 12. }));
                d = if row {
                    d.flex_row().items_start()
                } else {
                    d.flex_col()
                };
                // A row of tables in a column is as tall as its tallest; only a row with a stream takes the room left.
                let d = if row && dir == Some(NodeKind::Col) && !n.has_stream() {
                    d.flex_none()
                } else {
                    share(d, n)
                };
                d.children(
                    n.children
                        .iter()
                        .map(|c| self.node(c, Some(n.kind.clone()))),
                )
                .into_any_element()
            }
            NodeKind::Table => self.table(n),
            NodeKind::Stat => self.stats(n),
            NodeKind::Detail => self.detail(n),
            NodeKind::Stream => self.stream(n),
            NodeKind::Actions => self.actions(n),
            NodeKind::Note => share(card(t), n)
                .child(heading(t, &n.title, None))
                .child(div().text_color(t.dim).child(text(&n.text)))
                .into_any_element(),
            NodeKind::Chart => self.chart(n),
            NodeKind::Other(_) => div().into_any_element(),
        }
    }

    /// Each column as wide as its widest cell, the last taking the rest (the board's `minmax(5ch, max-content)`).
    fn widths(n: &Node) -> Vec<Option<f32>> {
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

    fn cells(
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
            .text_size(px(11.))
            .font_weight(w(600.))
    }

    fn row_cells(&self, r: &Row) -> Vec<(SharedString, gpui::Hsla)> {
        r.cells
            .iter()
            .map(|c| (SharedString::from(c.text.clone()), ui::ink(self.t, c.ink)))
            .collect()
    }

    fn table(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let widths = Self::widths(n);
        let focused = self.focused.as_deref() == Some(n.id.as_str());
        let mut rows = div().flex().flex_col();
        if n.rows.is_empty() {
            rows = rows.child(
                div()
                    .px(px(12.))
                    .min_h(px(34.))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(t.line)
                    .text_color(t.dim)
                    .child("nothing yet"),
            );
        }
        for (i, r) in n.rows.iter().enumerate() {
            let on = self.live.is_some() && i == n.selected;
            let mut row = self
                .cells(n, &widths, self.row_cells(r))
                .id(SharedString::from(format!("{}-{i}", n.id)))
                .min_h(px(34.))
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
            rows = rows.child(row);
        }
        share(div(), n)
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(if focused { t.line2 } else { t.line })
            .bg(t.panel)
            .overflow_hidden()
            .child(self.header(n, &widths))
            .child(rows)
            .into_any_element()
    }

    fn stats(&self, n: &Node) -> AnyElement {
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
                    .child(tracked(&s.label, 11., 400., t.dim, 0.66))
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(w(600.))
                            .text_color(match s.ink {
                                doze_desk::page::Ink::Text => t.bright,
                                i => ui::ink(t, i),
                            })
                            .child(text(&s.value)),
                    )
            }))
            .into_any_element()
    }

    fn detail(&self, n: &Node) -> AnyElement {
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
                            .text_size(px(12.))
                            .line_height(px(19.))
                            .text_color(t.bright)
                            .child(text(last)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.dim)
                            .child(text(&facts.join(" · "))),
                    );
            }
        }
        c.into_any_element()
    }

    fn stream(&self, n: &Node) -> AnyElement {
        let t = *self.t;
        let widths = Self::widths(n);
        let title = div()
            .px(px(12.))
            .min_h(px(28.))
            .flex()
            .items_center()
            .bg(t.well)
            .child(tracked(&n.title, 11., 600., t.dim, 0.55));
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
                    };
                    range
                        .filter_map(|i| rows.get(i))
                        .map(|r| {
                            d.cells(&node, &widths, d.row_cells(r))
                                .min_h(px(28.))
                                .border_t_1()
                                .border_color(t.line)
                                .font_family(MONO_FAMILY)
                                .text_size(px(12.))
                        })
                        .collect::<Vec<_>>()
                },
            )
            .flex_1()
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

    fn actions(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let mut c = share(card(t), n).child(heading(t, &n.title, None));
        if n.actions.is_empty() {
            c = c.child(div().text_color(t.dim).child("none"));
        }
        for (i, a) in n.actions.iter().enumerate() {
            let mut row = div()
                .id(("cact", i))
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(30.))
                .px(px(8.))
                .mx(px(-8.))
                .rounded(px(7.))
                .text_color(if a.destructive { t.bad } else { t.text })
                .hover(|s| s.bg(t.sel))
                .child(kbd_chip(t, &a.key.to_string(), 11., 600., 6.))
                .child(text(&a.label))
                .when(!a.resource.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.dim)
                            .child(text(&format!("on {}", a.resource))),
                    )
                });
            if let Some(e) = &self.live {
                let (e, a) = (e.clone(), a.clone());
                row = row
                    .cursor_pointer()
                    .on_click(move |_, _, cx| e.update(cx, |v, cx| v.act(a.clone(), cx)));
            }
            c = c.child(row);
        }
        c.into_any_element()
    }

    fn chart(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let data: Vec<(usize, f64)> = n.values.iter().copied().enumerate().collect();
        let line: AnyElement = if data.len() > 1 {
            AreaChart::new(data)
                .id(SharedString::from(format!("chart-{}", n.id)))
                .x(|(i, _)| SharedString::from(i.to_string()))
                .y(|(_, v)| *v)
                .stroke(t.accent)
                .fill(alpha(t.accent, 0.10))
                .linear()
                .x_axis(false)
                .grid(false)
                .into_any_element()
        } else {
            div().into_any_element()
        };
        share(card(t), n)
            .child(heading(t, &n.title, None))
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(w(600.))
                    .text_color(t.bright)
                    .child(text(if n.last.is_empty() { "—" } else { &n.last })),
            )
            .child(div().h(px(40.)).w_full().child(line))
            .into_any_element()
    }
}
