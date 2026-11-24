//! A module's console tab (PLAN.md §2.6): the tree of rows, columns and blocks the module published, each resolved by
//! doze-desk for the rows selected here, laid straight on the page (each block its own card, no frame around them).
//! `↑` `↓` (or `j` `k`) move in the focused table, `[` `]` move between tables, an action's capital letter runs it on
//! the selected row, a click on a column's title orders the table by it and `/` filters the focused table. The stream
//! follows the selected row: the hub's console subscription is told whenever that changes. Rates are worked out by the
//! hub as the tables arrive.

mod acts;
pub(crate) mod blocks;
mod figures;
mod filter;
mod parts;
mod stream;

use doze_desk::page::console::{self, Action, ConsolePage, FILTER_FROM, Picked, Selection, Views};
use doze_desk::{Batch, Hub, Parts, StackKey};
use doze_display::NodeKind;
use gpui::{
    Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render,
    UniformListScrollHandle, Window, div, prelude::*, px,
};
use gpui_component::input::InputState;

use crate::actions::{FilterRows, NextTable, PrevTable, RowDown, RowUp};
use crate::instance::head::{chip, keys_bar};
use crate::instance::ui::{self, text};
use crate::theme::{tokens, ty};

/// The height each stream's place in the layout had when last laid out, by block id: a stream shows as many whole lines
/// as fit (`Draw::stream`).
pub(crate) type Rooms = std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, f32>>>;

/// One console tab.
pub(crate) struct ConsoleTabView {
    hub: Hub,
    key: StackKey,
    name: String,
    tab: String,
    /// The row picked in each table, by name: what a person chose, kept through the module's re-reads.
    pub(crate) picked: Picked,
    /// Those picks as rows of the tables last read (`console::selection`).
    pub(crate) sel: Selection,
    /// How each table is ordered and filtered, by block id.
    pub(crate) views: Views,
    /// The table the arrows move in, by block id; the first table when none was picked.
    pub(crate) focused: Option<String>,
    pub(crate) page: ConsolePage,
    followed: (String, String),
    pub(crate) stream: UniformListScrollHandle,
    /// The scroll handles of the tables too long to lay out whole, by block id.
    pub(crate) lists: std::collections::HashMap<String, UniformListScrollHandle>,
    /// The filter box, and the table it filters, while one is open.
    pub(crate) filter: Option<(String, Entity<InputState>)>,
    pub(crate) rooms: Rooms,
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
            picked: Picked::new(),
            sel: Selection::new(),
            views: Views::new(),
            focused: None,
            page: ConsolePage::default(),
            followed: Default::default(),
            stream: UniformListScrollHandle::new(),
            lists: std::collections::HashMap::new(),
            filter: None,
            rooms: Rooms::default(),
            focus: cx.focus_handle(),
        };
        v.refresh(cx);
        v
    }

    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        if batch
            .parts(&self.key)
            .intersects(Parts::CONSOLE | Parts::INSTANCES | Parts::STATS)
        {
            self.refresh(cx);
        }
    }

    /// Resolves the tab for the selection; tells the hub which stream row to follow when that changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let st = self.hub.stack(&self.key);
        let c = st.as_ref().and_then(|s| s.consoles.get(&self.name));
        let mut gone = Vec::new();
        if let Some(c) = c {
            self.sel = console::selection(c, &self.tab, &self.picked);
            gone = console::gone(c, &self.tab, &self.picked);
        }
        let mut page =
            console::console_page_viewed(&self.name, &self.tab, c, &self.sel, &self.views);
        let follow = c
            .map(|c| console::follow(c, &self.tab, &self.sel))
            .unwrap_or_default();
        if follow != self.followed && !follow.0.is_empty() {
            self.hub
                .follow_console(&self.key, &self.name, &follow.0, &follow.1);
            self.followed = follow;
        }
        // A table whose picked row is gone (the session ended) has nothing picked until a person picks again: no row
        // is drawn as selected, its detail says nothing is, and nothing can be done to "the selected row" there.
        for id in &gone {
            acts::unpick(&mut page.body, id);
        }
        if page != self.page {
            // A new line, not a longer stream: a full stream (`STREAM_KEPT`) drops its oldest as each comes.
            let grew = streams(&page) != streams(&self.page);
            for n in page.body.walk() {
                if n.kind == NodeKind::Table && n.total > parts::BIG {
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

    /// The tables of the tab, in order: their ids and the rows they show, as indexes among the module's rows.
    fn tables(&self) -> Vec<(String, Vec<usize>)> {
        self.page
            .body
            .walk()
            .into_iter()
            .filter(|n| n.kind == NodeKind::Table)
            .map(|n| (n.id.clone(), n.rows.iter().map(|r| r.at).collect()))
            .collect()
    }

    pub(crate) fn focused_table(&self) -> Option<String> {
        let tables = self.tables();
        self.focused
            .clone()
            .filter(|f| tables.iter().any(|(id, _)| id == f))
            .or_else(|| tables.first().map(|(id, _)| id.clone()))
    }

    /// Picks the module's row `at` of the table `id`, by its name.
    pub(crate) fn pick(&mut self, id: &str, at: usize, cx: &mut Context<Self>) {
        self.focused = Some(id.to_owned());
        let name = self
            .page
            .body
            .walk()
            .into_iter()
            .find(|n| n.id == id)
            .and_then(|n| n.rows.iter().find(|r| r.at == at))
            .map(|r| r.ident.clone());
        if let Some(name) = name {
            // Picking again is how a table whose pick went comes back.
            self.picked.insert(id.to_owned(), name);
        }
        self.sel.insert(id.to_owned(), at);
        self.refresh(cx);
        self.reveal(id);
        cx.notify();
    }

    /// Scrolls a long table so its selected row is in view, wherever its order put it.
    pub(crate) fn reveal(&self, id: &str) {
        let at = self.sel.get(id).copied().unwrap_or(0);
        let shown = self.tables().into_iter().find(|(t, _)| t == id);
        if let (Some(h), Some((_, rows))) = (self.lists.get(id), shown)
            && let Some(i) = rows.iter().position(|&r| r == at)
        {
            h.scroll_to_item(i, gpui::ScrollStrategy::Center);
        }
    }

    /// Moves the selection `by` rows in the order the table shows them.
    fn step_row(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(id) = self.focused_table() else {
            return;
        };
        let Some((_, rows)) = self.tables().into_iter().find(|(t, _)| *t == id) else {
            return;
        };
        if rows.is_empty() {
            return;
        }
        let at = self.sel.get(&id).copied().unwrap_or(0);
        let pos = rows.iter().position(|&r| r == at);
        let next = match pos {
            Some(p) => (p as isize + by).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        if let Some(&at) = rows.get(next) {
            self.pick(&id, at, cx);
        }
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

    /// A capital letter runs the action it names, on the selected row (not while the filter box has the keys).
    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let typing = self
            .filter
            .as_ref()
            .is_some_and(|(_, i)| crate::widgets::has_keys(i, window, cx));
        // Asleep, the console never wakes the module: its actions wait (dimmed on the page).
        let asleep = !self.page.state.is_empty() && !self.page.awake;
        if typing
            || asleep
            || k.modifiers.platform
            || k.modifiers.control
            || k.modifiers.alt
            || !k.modifiers.shift
        {
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

    /// Every action the tab offers, once: those of the actions blocks and those at a table's selected row.
    fn actions(&self) -> Vec<Action> {
        let mut out: Vec<Action> = Vec::new();
        for a in self
            .page
            .body
            .walk()
            .into_iter()
            .flat_map(|n| n.actions.iter().chain(&n.row_actions))
        {
            if !out.iter().any(|b| b.id == a.id) {
                out.push(a.clone());
            }
        }
        out
    }

    /// Runs a module's action on the row it shows; one that takes input or deletes something asks first
    /// (`crate::act`). The tables are read again first: an action whose row has gone since it was drawn (the session
    /// ended) is not run on whatever row took its place, and says so.
    pub(crate) fn act(&mut self, a: Action, cx: &mut Context<Self>) {
        self.refresh(cx);
        let now = self.actions().into_iter().find(|b| b.id == a.id);
        let acts = crate::act::acts(cx);
        // The same row, by identity, as the one it was started on; a row that is gone is never swapped for another.
        let same = now.is_some_and(|b| b.row == a.row && b.resource == a.resource);
        if !same {
            let said = format!(
                "{} was not run: {} is no longer there.",
                a.label, a.resource
            );
            if let Some(acts) = acts {
                acts.update(cx, |acts, cx| acts.say(said, true, cx));
            }
            return;
        }
        let (key, name) = (self.key.clone(), self.name.clone());
        if let Some(acts) = acts {
            acts.update(cx, |acts, cx| acts.action(&key, &name, &a, cx));
        }
    }

    /// A table long enough to want a filter.
    fn long_table(&self) -> bool {
        self.page
            .body
            .walk()
            .iter()
            .any(|n| n.kind == NodeKind::Table && n.total > FILTER_FROM)
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
    /// What the tab holds: the tree, or why there is none yet.
    fn body(
        &self,
        t: &crate::theme::Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = &self.page;
        let why = if p.reading && !p.err.is_empty() {
            // The banner above says why; not "reading", which it is not.
            Some("Nothing to show until the module answers.")
        } else if p.reading {
            Some("Reading the console…")
        } else if p.missing {
            Some("The module has no such tab.")
        } else {
            None
        };
        match why {
            // Said where the blocks would start, not floating mid-page.
            Some(w) => div()
                .flex_1()
                .px(px(28.))
                .pt(px(16.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(w)
                .into_any_element(),
            None => blocks::tab_body(t, self, window, cx),
        }
    }

    /// Asleep, the console shows what it last read and never wakes the module: one quiet line says so.
    fn asleep(&self, t: &crate::theme::Tokens) -> Option<gpui::Div> {
        let p = &self.page;
        (!p.state.is_empty() && !p.awake && !p.missing && !p.reading).then(|| {
            div()
                .flex_none()
                .px(px(28.))
                .pt(px(12.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(text(&format!(
                    "{} is {}: the console never wakes it, so this is what it showed last.",
                    p.name,
                    doze_display::State::parse(&p.state).word()
                )))
        })
    }
}

impl Render for ConsoleTabView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let body = self.body(&t, window, cx);
        let p = &self.page;
        let mut keys: Vec<(String, &str)> = p
            .keys
            .iter()
            .map(|(k, w)| (k.clone(), w.as_str()))
            .collect();
        // Asleep, the actions wait (dimmed on the page): their keys are not offered.
        if !p.state.is_empty() && !p.awake {
            keys.retain(|(_, w)| *w == "select" || *w == "next tab");
        }
        // Between tables only where there are two; the filter where one is long.
        let mut at = usize::from(keys.first().is_some_and(|k| k.1 == "select"));
        if self.tables().len() > 1 {
            keys.insert(
                at,
                (
                    format!("{} {}", chip("Table before"), chip("Next table")),
                    "tables",
                ),
            );
            at += 1;
        }
        if self.long_table() {
            keys.insert(at, (chip("Filter the table"), "filter"));
        }
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
            .on_action(cx.listener(|v, _: &FilterRows, w, cx| v.open_filter(w, cx)))
            .capture_key_down(cx.listener(|v, e: &KeyDownEvent, w, cx| {
                if e.keystroke.key == "escape" && v.filter.is_some() {
                    v.close_filter(w, cx);
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|v, e: &KeyDownEvent, w, cx| v.key_down(e, w, cx)))
            .when(!p.err.is_empty(), |d| {
                d.child(div().px(px(28.)).pt(px(14.)).child(ui::banner(
                    &t,
                    ui::Tone::Bad,
                    &sentence(&p.err),
                    "",
                )))
            })
            .children(self.asleep(&t))
            .child(body)
            .child(keys_bar(&t, keys, &p.note))
    }
}

/// Words as a sentence starts: a capital first ("the module did not answer" is "The module did not answer").
fn sentence(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
