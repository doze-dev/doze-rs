//! A console's node tree drawn: rows and columns share their room by weight; a table's rows pick, a stream keeps its
//! newest line in view, an action runs by its letter. The same drawing serves a module's blocks on the overview,
//! without the picking and without a card of their own (the overview's card is their frame).

use doze_desk::page::console::{Action, Node};
use doze_display::NodeKind;
use gpui::{
    AnyElement, Context, Div, Entity, IntoElement, SharedString, Window, div, prelude::*, px,
    relative,
};
use gpui_component::chart::AreaChart;
use gpui_component::input::InputState;

use super::ConsoleTabView;
use super::parts::BIG;
use crate::instance::ui::{card, heading, text};
use crate::theme::{Tokens, alpha, ty};
use crate::widgets::kbd_chip;

/// What a drawing can do: pick rows, order and filter tables, and run actions on a console tab; nothing on the
/// overview.
pub(super) struct Draw<'a> {
    pub(super) t: &'a Tokens,
    pub(super) focused: Option<String>,
    pub(super) live: Option<Entity<ConsoleTabView>>,
    pub(super) stream: Option<gpui::UniformListScrollHandle>,
    /// The long tables' scroll handles, by block id, so the picked row stays in view.
    pub(super) lists: Option<&'a std::collections::HashMap<String, gpui::UniformListScrollHandle>>,
    /// The open filter box: the table it filters, the box, and whether the keys are in it.
    pub(super) filter: Option<(String, Entity<InputState>, bool)>,
    /// On the overview: inside the overview's own card, so a block draws no card of its own.
    pub(super) flat: bool,
}

/// The tab's tree, live.
pub(crate) fn tab_body(
    t: &Tokens,
    v: &ConsoleTabView,
    window: &Window,
    cx: &mut Context<ConsoleTabView>,
) -> AnyElement {
    let d = Draw {
        t,
        focused: v.focused_table(),
        live: Some(cx.entity()),
        stream: Some(v.stream.clone()),
        lists: Some(&v.lists),
        filter: v.filter.as_ref().map(|(id, i)| {
            (
                id.clone(),
                i.clone(),
                crate::widgets::has_keys(i, window, cx),
            )
        }),
        flat: false,
    };
    div()
        .id("con")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .px(px(28.))
        .pt(px(16.))
        .pb(px(14.))
        .overflow_y_scroll()
        // Its width is the page's: a long note or a wide table wraps or scrolls inside it, never widens the page.
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(d.node(&v.page.body, None)),
        )
        .into_any_element()
}

/// A block on the overview: drawn, not picked, inside the overview's card.
pub(crate) fn static_block(t: &Tokens, n: &Node) -> AnyElement {
    let d = Draw {
        t,
        focused: None,
        live: None,
        stream: None,
        lists: None,
        filter: None,
        flat: true,
    };
    // The overview's cards are a column's width (half the page): a row the module laid out for a whole tab stacks
    // there, each block full width, rather than three blocks squeezed to a third each.
    let stacked;
    let n = if n.kind == NodeKind::Row {
        stacked = Node {
            kind: NodeKind::Col,
            ..n.clone()
        };
        &stacked
    } else {
        n
    };
    div().min_w_0().child(d.node(n, None)).into_any_element()
}

/// Lets a block take its share of its parent's room, or only what it needs.
pub(super) fn share(d: Div, n: &Node) -> Div {
    if n.flexible {
        d.flex_basis(relative(0.))
            .flex_grow(n.weight.max(1) as f32)
            .min_w_0()
            .min_h_0()
    } else {
        d.flex_none().min_w_0()
    }
}

/// A card of words in a row takes its share of the row's width even when the module gave it no weight: text laid out
/// at its own width is one long line, which would push the row past the window.
fn words(d: Div, n: &Node, dir: Option<&NodeKind>) -> Div {
    if n.flexible || dir != Some(&NodeKind::Row) {
        d
    } else {
        d.flex_1().flex_basis(relative(0.))
    }
}

/// It wants the height left over: a stream with lines, or a table too long to lay out whole.
pub(super) fn grows(n: &Node) -> bool {
    n.walk().iter().any(|b| {
        (b.kind == NodeKind::Stream && !b.rows.is_empty())
            || (b.kind == NodeKind::Table && b.total > BIG)
    })
}

impl Draw<'_> {
    /// A block's frame: a card on a tab; on the overview, whose card holds it, a plain box with the card's padding.
    pub(super) fn frame(&self) -> Div {
        if self.flat {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .min_w_0()
                .px(px(14.))
                .py(px(10.))
        } else {
            card(self.t)
        }
    }

    pub(super) fn node(&self, n: &Node, dir: Option<NodeKind>) -> AnyElement {
        let t = self.t;
        if !n.missing.is_empty() {
            return words(share(self.frame(), n), n, dir.as_ref())
                .text_color(t.dim)
                .child(text(&n.missing))
                .into_any_element();
        }
        match n.kind {
            NodeKind::Row | NodeKind::Col => {
                let row = n.kind == NodeKind::Row;
                let mut d = div().flex().gap(px(if self.flat {
                    0.
                } else if row {
                    14.
                } else {
                    12.
                }));
                let tall = grows(n);
                d = if row {
                    // A row that grows stretches its blocks to its height (a long table beside its detail); else each
                    // is as tall as its content.
                    if tall {
                        d.flex_row()
                    } else {
                        d.flex_row().items_start()
                    }
                } else {
                    d.flex_col()
                };
                // A row in a column is as tall as its tallest block; only one holding a stream with lines or a long
                // table takes the room left.
                let d = if row && dir == Some(NodeKind::Col) && !tall {
                    // min_w_0: text measures its whole line as its least width; a row of cards must not take that.
                    d.flex_none().min_w_0()
                } else if !tall && dir == Some(NodeKind::Col) {
                    d.flex_none().min_w_0()
                } else {
                    share(d, n)
                };
                d.children(n.children.iter().filter(|c| !hidden(c)).map(|c| {
                    let drawn = self.node(c, Some(n.kind.clone()));
                    // Across a row, a block of words or figures takes its weight's share of the width, as tables do:
                    // drawn at its own width, a chart is a stub and a note one long line.
                    if row && !c.flexible {
                        div()
                            .flex()
                            .flex_col()
                            .flex_basis(relative(0.))
                            .flex_grow(c.weight.max(1) as f32)
                            .min_w_0()
                            .child(drawn)
                            .into_any_element()
                    } else {
                        drawn
                    }
                }))
                .into_any_element()
            }
            NodeKind::Table => self.table(n, dir.as_ref()),
            NodeKind::Stat => self.stats(n),
            NodeKind::Detail => self.detail(n),
            NodeKind::Stream => self.stream(n),
            NodeKind::Actions => self.actions(n),
            NodeKind::Note => words(share(self.frame(), n), n, dir.as_ref())
                .child(heading(t, &n.title, None))
                .child(div().text_color(t.dim).child(text(&n.text)))
                .into_any_element(),
            NodeKind::Chart => self.chart(n),
            // The resolver says what it is (`missing`); this is for a tree resolved by an older doze-desk.
            NodeKind::Other(_) => words(share(self.frame(), n), n, dir.as_ref())
                .text_color(t.dim)
                .child("This needs a newer doze.")
                .into_any_element(),
        }
    }

    /// One action as a line: its letter, its label, the row it acts on. On a tab, a click runs it.
    pub(super) fn action_line(&self, a: &Action, i: usize, scope: &str) -> AnyElement {
        let t = self.t;
        let mut row = div()
            .id(SharedString::from(format!("act-{scope}-{i}")))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(30.))
            .px(px(8.))
            .mx(px(-8.))
            .rounded(px(7.))
            .text_color(if a.destructive { t.bad } else { t.text })
            .child(kbd_chip(t, &a.key.to_string(), ty::XS, ty::SEMIBOLD, 6.))
            .child(text(&a.label))
            .when(!a.resource.is_empty(), |d| {
                d.child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(ty::SM))
                        .text_color(t.dim)
                        .child(text(&format!("on {}", a.resource))),
                )
            });
        if let Some(e) = &self.live {
            let (e, a) = (e.clone(), a.clone());
            row = row
                .cursor_pointer()
                .hover(|s| s.bg(t.sel))
                .on_click(move |_, _, cx| e.update(cx, |v, cx| v.act(a.clone(), cx)));
        }
        row.into_any_element()
    }

    fn actions(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let mut c = share(self.frame(), n).child(heading(t, &n.title, None));
        if n.actions.is_empty() {
            c = c.child(div().text_color(t.dim).child("none"));
        }
        for (i, a) in n.actions.iter().enumerate() {
            c = c.child(self.action_line(a, i, &n.id));
        }
        c.into_any_element()
    }

    fn chart(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let data: Vec<(usize, f64)> = n.values.iter().copied().enumerate().collect();
        let line: AnyElement = if data.len() > 1 {
            AreaChart::new(data)
                .id(SharedString::from(format!("chart-{}", n.id)))
                .interactive(!crate::motion::still())
                .appear(!crate::motion::still())
                .x(|(i, _)| SharedString::from(i.to_string()))
                .y(|(_, v)| *v)
                .stroke(t.cat(0))
                .fill(alpha(t.cat(0), 0.10))
                .linear()
                .x_axis(false)
                .grid(false)
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let small = |s: &str| {
            div()
                .text_size(px(ty::XS))
                .text_color(t.dim)
                .font_features(ty::tabular())
                .whitespace_nowrap()
                .child(text(s))
        };
        // A light scale: the greatest reading at the top, the least at the bottom, and what x runs over below.
        let scale = div()
            .flex()
            .flex_col()
            .flex_none()
            .justify_between()
            .items_end()
            .h(px(56.))
            .child(small(&n.high))
            .child(small(&n.low));
        share(self.frame(), n)
            .child(heading(t, &n.title, None))
            .child(
                div()
                    .text_size(px(ty::FIGURE))
                    .font_weight(ty::SEMIBOLD)
                    .text_color(t.bright)
                    .font_features(ty::tabular())
                    .child(text(if n.last.is_empty() { "—" } else { &n.last })),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .w_full()
                    .when(!n.high.is_empty(), |d| d.child(scale))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(56.))
                            .border_b_1()
                            .border_color(t.line)
                            .child(line),
                    ),
            )
            .when(!n.span.is_empty(), |d| {
                d.child(div().flex().justify_end().child(small(&n.span)))
            })
            .into_any_element()
    }
}

/// An actions block whose every action went to a table's selected row: nothing left to draw.
fn hidden(n: &Node) -> bool {
    n.kind == NodeKind::Actions && n.moved && n.actions.is_empty()
}
