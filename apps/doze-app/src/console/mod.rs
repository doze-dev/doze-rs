//! A module's console tab (PLAN.md §2.6), as the ConsoleTab board frames it: one frame with a top line saying what
//! is shown, and inside it the tree of rows, columns and blocks the module published, each resolved by doze-desk for
//! the rows selected here. `↑` `↓` (or `j` `k`) move in the focused table, `[` `]` move between tables, an action's
//! capital letter runs it on the selected row. The stream follows the selected row: the hub's console subscription is
//! told whenever that changes. Rates are worked out by the hub as the tables arrive.

pub(crate) mod blocks;
mod parts;

use doze_desk::page::console::{self, Action, ConsolePage, Selection};
use doze_desk::{Batch, Hub, Parts, StackKey};
use doze_display::NodeKind;
use gpui::{
    Context, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, UniformListScrollHandle,
    Window, div, prelude::*, px,
};

use crate::actions::{NextTable, PrevTable, RowDown, RowUp};
use crate::instance::head::{chip, keys_bar};
use crate::instance::ui::{self, text};
use crate::theme::tokens;
use crate::widgets::w;

/// One console tab.
pub(crate) struct ConsoleTabView {
    hub: Hub,
    key: StackKey,
    name: String,
    tab: String,
    pub(crate) sel: Selection,
    /// The table the arrows move in, by block id; the first table when none was picked.
    pub(crate) focused: Option<String>,
    pub(crate) page: ConsolePage,
    followed: (String, String),
    pub(crate) stream: UniformListScrollHandle,
    /// The scroll handles of the tables too long to lay out whole, by block id.
    pub(crate) lists: std::collections::HashMap<String, UniformListScrollHandle>,
    pub(crate) focus: FocusHandle,
}

impl ConsoleTabView {
    pub(crate) fn new(
        hub: Hub,
        key: StackKey,
        name: &str,
        tab: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut v = ConsoleTabView {
            hub,
            key,
            name: name.to_owned(),
            tab: tab.to_owned(),
            sel: Selection::new(),
            focused: None,
            page: ConsolePage::default(),
            followed: Default::default(),
            stream: UniformListScrollHandle::new(),
            lists: std::collections::HashMap::new(),
            focus: cx.focus_handle(),
        };
        v.refresh(cx);
        v
    }

    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        if batch
            .parts(&self.key)
            .intersects(Parts::CONSOLE | Parts::INSTANCES)
        {
            self.refresh(cx);
        }
    }

    /// Resolves the tab for the selection; tells the hub which stream row to follow when that changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let st = self.hub.stack(&self.key);
        let c = st.as_ref().and_then(|s| s.consoles.get(&self.name));
        let page = console::console_page(&self.name, &self.tab, c, &self.sel);
        let follow = c
            .map(|c| console::follow(c, &self.tab, &self.sel))
            .unwrap_or_default();
        if follow != self.followed && !follow.0.is_empty() {
            self.hub
                .follow_console(&self.key, &self.name, &follow.0, &follow.1);
            self.followed = follow;
        }
        if page != self.page {
            // A new line, not a longer stream: a full stream (`STREAM_KEPT`) drops its oldest as each comes.
            let grew = streams(&page) != streams(&self.page);
            for n in page.body.walk() {
                if n.kind == NodeKind::Table && n.rows.len() > parts::BIG {
                    self.lists.entry(n.id.clone()).or_default();
                }
            }
            self.page = page;
            if grew {
                self.stream.scroll_to_bottom();
            }
            cx.notify();
        }
    }

    /// The tables of the tab, in order: their ids and row counts.
    fn tables(&self) -> Vec<(String, usize)> {
        self.page
            .body
            .walk()
            .into_iter()
            .filter(|n| n.kind == NodeKind::Table)
            .map(|n| (n.id.clone(), n.rows.len()))
            .collect()
    }

    pub(crate) fn focused_table(&self) -> Option<String> {
        let tables = self.tables();
        self.focused
            .clone()
            .filter(|f| tables.iter().any(|(id, _)| id == f))
            .or_else(|| tables.first().map(|(id, _)| id.clone()))
    }

    /// Picks row `i` of the table `id`.
    pub(crate) fn pick(&mut self, id: &str, i: usize, cx: &mut Context<Self>) {
        self.focused = Some(id.to_owned());
        self.sel.insert(id.to_owned(), i);
        if let Some(h) = self.lists.get(id) {
            h.scroll_to_item(i, gpui::ScrollStrategy::Center);
        }
        self.refresh(cx);
        cx.notify();
    }

    fn step_row(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(id) = self.focused_table() else {
            return;
        };
        let n = self
            .tables()
            .into_iter()
            .find(|(t, _)| *t == id)
            .map_or(0, |(_, n)| n);
        let at = self.sel.get(&id).copied().unwrap_or(0) as isize;
        let next = (at + by).clamp(0, n.saturating_sub(1) as isize) as usize;
        self.pick(&id, next, cx);
    }

    fn step_table(&mut self, by: isize, cx: &mut Context<Self>) {
        let tables = self.tables();
        let Some(cur) = self.focused_table() else {
            return;
        };
        let i = tables.iter().position(|(t, _)| *t == cur).unwrap_or(0) as isize;
        let n = tables.len() as isize;
        if let Some((id, _)) = tables.get(((i + by).rem_euclid(n.max(1))) as usize) {
            self.focused = Some(id.clone());
            cx.notify();
        }
    }

    /// A capital letter runs the action it names, on the selected row.
    fn key_down(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if k.modifiers.platform || k.modifiers.control || k.modifiers.alt || !k.modifiers.shift {
            return;
        }
        let Some(c) = k.key.chars().next().filter(|_| k.key.chars().count() == 1) else {
            return;
        };
        let c = c.to_ascii_uppercase();
        let found = self.actions().into_iter().find(|a| a.key == c);
        if let Some(a) = found {
            self.act(a, cx);
            cx.stop_propagation();
        }
    }

    fn actions(&self) -> Vec<Action> {
        self.page
            .body
            .walk()
            .into_iter()
            .flat_map(|n| n.actions.clone())
            .collect()
    }

    /// Runs a module's action; one that takes input or deletes something asks first (`crate::act`).
    pub(crate) fn act(&mut self, a: Action, cx: &mut Context<Self>) {
        let (key, name) = (self.key.clone(), self.name.clone());
        if let Some(acts) = crate::act::acts(cx) {
            acts.update(cx, |acts, cx| acts.action(&key, &name, &a, cx));
        }
    }
}

/// Each stream's length and newest row, to keep the newest in view as rows arrive (the length alone stops moving once
/// a stream is full, `STREAM_KEPT`).
fn streams(p: &ConsolePage) -> Vec<(usize, Option<doze_desk::page::console::Row>)> {
    p.body
        .walk()
        .into_iter()
        .filter(|n| n.kind == NodeKind::Stream)
        .map(|n| (n.rows.len(), n.rows.last().cloned()))
        .collect()
}

impl Focusable for ConsoleTabView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConsoleTabView {
    /// The frame's top line: whose console, which tab, how it is read.
    fn top(&self, t: &crate::theme::Tokens) -> gpui::Div {
        let p = &self.page;
        let title = self
            .tab
            .strip_prefix("console:")
            .filter(|s| !s.is_empty() && *s != "_")
            .unwrap_or("console");
        let mut top = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .min_h(px(44.))
            .px(px(16.))
            .border_b_1()
            .border_color(t.line)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_color(t.bright)
            .font_weight(w(600.))
            .child(text(&self.name))
            .child(
                div()
                    .font_weight(w(400.))
                    .text_color(t.dim)
                    .child(text(&format!("· {title}"))),
            );
        if !p.state.is_empty() && !p.awake && !p.missing {
            top = top.child(
                div()
                    .font_weight(w(400.))
                    .text_color(t.dim)
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(text(&format!(
                        "· {}: the console never wakes it; what it last showed is kept",
                        p.state
                    ))),
            );
        }
        top = top.child(div().flex_1()).child(
            div()
                .font_weight(w(400.))
                .text_size(px(12.))
                .text_color(t.dim)
                .overflow_hidden()
                .text_ellipsis()
                .child(text(&p.note)),
        );
        top
    }

    /// What the frame holds: the tree, or why there is none yet.
    fn body(&self, t: &crate::theme::Tokens, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = &self.page;
        if p.reading {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(t.dim)
                .child("reading the console…")
                .into_any_element()
        } else if p.missing {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(t.dim)
                .child("the module has no such tab")
                .into_any_element()
        } else {
            blocks::tab_body(t, self, cx)
        }
    }
}

impl Render for ConsoleTabView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let (top, body) = (self.top(&t), self.body(&t, cx));
        let p = &self.page;
        let mut keys: Vec<(String, &str)> = p
            .keys
            .iter()
            .map(|(k, w)| (k.clone(), w.as_str()))
            .collect();
        keys.insert(
            1.min(keys.len()),
            (
                format!("{} {}", chip("Table before"), chip("Next table")),
                "tables",
            ),
        );
        div()
            .id("console")
            .key_context("ConsoleTab")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .on_action(cx.listener(|v, _: &RowDown, _, cx| v.step_row(1, cx)))
            .on_action(cx.listener(|v, _: &RowUp, _, cx| v.step_row(-1, cx)))
            .on_action(cx.listener(|v, _: &NextTable, _, cx| v.step_table(1, cx)))
            .on_action(cx.listener(|v, _: &PrevTable, _, cx| v.step_table(-1, cx)))
            .on_key_down(cx.listener(|v, e: &KeyDownEvent, _, cx| v.key_down(e, cx)))
            .when(!p.err.is_empty(), |d| {
                d.child(div().px(px(28.)).pt(px(14.)).child(ui::banner(
                    &t,
                    ui::Tone::Bad,
                    &p.err,
                    "",
                )))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .mx(px(28.))
                    .mt(px(0.))
                    .mb(px(3.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(t.line)
                    .bg(t.well)
                    .overflow_hidden()
                    .child(top)
                    .child(body),
            )
            .child(keys_bar(
                &t,
                keys,
                "from the Overview tab: Console, or ⏎ on an asleep module",
            ))
    }
}
