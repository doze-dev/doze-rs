//! A stack's page (the Tiles, List, Map and Grid boards; Go `pages/overview.tsx`): the head with the view switch and
//! the stack's switch, one strip of figures, then the stack in the view remembered for it. A stopped stack is the same
//! page in another state: the same rows in the same places, each down, the strip saying what Up starts.
//!
//! Every word is doze-desk's ([`doze_desk::overview`]); this keeps the selection, the filter, the Grid's crosshair.
//! The page watches the stack's meters while it shows, so its figures are the last second's; it redraws only when what
//! it shows changed.

mod grid;
mod head;
mod keys;
mod list;
mod pane;
mod strip;
mod tiles;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use doze_control::Topic;
use doze_desk::overview::{Home, Pane, Refs, Row, home, pane, refs_in, stopped_home};
use doze_desk::{Hub, StackKey, StackView, Watch};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    Render, Window, div, prelude::*, px,
};
use gpui_component::input::{InputEvent, InputState};

use crate::actions::{
    CopyAddress, FilterRows, MoveDown, MoveLeft, MoveRight, MoveUp, OpenService, ToggleService,
    ViewGrid, ViewList, ViewMap, ViewTiles,
};
use crate::facts::Facts;
use crate::map::MapView;
use crate::model::{self, Page};
use crate::theme::{Density, tokens};

/// The overview's views, in the switch's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum View {
    #[default]
    Tiles,
    List,
    Map,
    Grid,
}

impl View {
    pub(crate) const ALL: [View; 4] = [View::Tiles, View::List, View::Map, View::Grid];

    pub(crate) fn word(self) -> &'static str {
        match self {
            View::Tiles => "tiles",
            View::List => "list",
            View::Map => "map",
            View::Grid => "grid",
        }
    }

    pub(crate) fn parse(w: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.word() == w)
    }
}

/// What the page asks of the window.
#[derive(Debug, Clone)]
pub(crate) enum OverviewEvent {
    Say(String),
}

/// A stack's page.
pub(crate) struct OverviewView {
    hub: Hub,
    facts: Entity<Facts>,
    pub(crate) key: Option<StackKey>,
    stack: Option<StackView>,
    pub(crate) home: Home,
    page: Page,
    pub(crate) map: Entity<MapView>,
    pub(crate) view: View,
    pub(crate) selected: Option<String>,
    pub(crate) pane: Option<Pane>,
    pub(crate) filter: Entity<InputState>,
    pub(crate) query: String,
    pub(crate) density: Density,
    /// The Grid's cell under the pointer or the keys: row, column.
    pub(crate) cur: Option<(usize, usize)>,
    refs: HashMap<String, Refs>,
    watch: Option<Watch>,
    /// The window's width at the last render (the Tiles go four, three or two across).
    pub(crate) width: f32,
    /// The List's lines, and its virtual list.
    items: std::rc::Rc<Vec<list::Item>>,
    list: gpui::ListState,
    pub(crate) focus: FocusHandle,
}

impl EventEmitter<OverviewEvent> for OverviewView {}

impl Focusable for OverviewView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl OverviewView {
    pub(crate) fn new(
        hub: Hub,
        facts: Entity<Facts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> OverviewView {
        cx.observe(&facts, |this, _, cx| this.refresh(cx)).detach();
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        cx.subscribe(
            &filter,
            |this: &mut OverviewView, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.query = input.read(cx).value().to_string();
                    cx.notify();
                }
            },
        )
        .detach();
        OverviewView {
            hub,
            facts,
            key: None,
            stack: None,
            home: Home::default(),
            page: Page::default(),
            map: cx.new(MapView::new),
            view: View::default(),
            selected: None,
            pane: None,
            filter,
            query: String::new(),
            density: Density::parse(&crate::prefs::density()).unwrap_or_default(),
            cur: None,
            refs: HashMap::new(),
            watch: None,
            width: 1280.,
            items: std::rc::Rc::default(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(200.)),
            focus: cx.focus_handle(),
        }
    }

    /// Shows the stack `key` (its meters watched while it shows), in the view remembered for it.
    pub(crate) fn show(&mut self, key: Option<StackKey>, cx: &mut Context<Self>) {
        if key == self.key && self.stack.is_some() {
            return;
        }
        self.watch = key.as_ref().map(|k| self.hub.watch(k, Topic::Stats));
        let remembered = key
            .as_ref()
            .and_then(|k| crate::prefs::view(&k.dir().display().to_string()));
        self.view = remembered
            .as_deref()
            .and_then(View::parse)
            .unwrap_or_default();
        self.key = key;
        self.stack = None;
        self.selected = None;
        self.cur = None;
        self.refresh(cx);
    }

    /// Stops watching the stack's meters (the page is not shown).
    pub(crate) fn hide(&mut self) {
        self.watch = None;
    }

    /// Reads the stack again; redraws only if the page changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.key.as_ref().and_then(|k| self.hub.stack(k)) else {
            return;
        };
        if self.watch.is_none() {
            self.watch = Some(self.hub.watch(&st.key, Topic::Stats));
        }
        let now = self.hub.now();
        let (h, map_view) = if st.running {
            let mut h = home(&st, now);
            h.changed = changed(&st, now);
            let refs = self
                .refs
                .entry(st.dir.clone())
                .or_insert_with(|| refs_in(Path::new(&st.dir)).unwrap_or_default());
            h.grid.refer(refs);
            (h, st.clone())
        } else {
            let shape = self
                .facts
                .update(cx, |f, cx| f.shape(&st.dir, cx))
                .unwrap_or_default();
            let mut as_up = st.clone();
            as_up.instances = Arc::new(shape.instances());
            (stopped_home(&st, &shape, now), as_up)
        };
        let page = model::page(&map_view, now);
        if page != self.page {
            self.map.update(cx, |m, cx| m.set_page(&page, cx));
            self.page = page;
        }
        let rows = all_rows(&h);
        if self
            .selected
            .as_ref()
            .is_none_or(|s| !rows.iter().any(|r| &r.name == s))
        {
            self.selected = rows.first().map(|r| r.name.clone());
        }
        let p = match (&self.selected, st.running) {
            (Some(s), true) => pane(&st, s, now),
            _ => None,
        };
        self.stack = Some(st);
        if h != self.home || p != self.pane {
            self.home = h;
            self.pane = p;
            cx.notify();
        }
    }

    pub(crate) fn set_view(&mut self, v: View, cx: &mut Context<Self>) {
        self.view = v;
        self.cur = None;
        if let Some(k) = &self.key {
            crate::prefs::save_view(&k.dir().display().to_string(), v.word());
        }
        cx.notify();
    }

    pub(crate) fn set_density(&mut self, d: Density, cx: &mut Context<Self>) {
        self.density = d;
        crate::prefs::save_density(d.word());
        cx.notify();
    }

    /// The rows the filter leaves, in the page's order.
    pub(crate) fn shown(&self) -> Vec<&Row> {
        let q = self.query.trim().to_lowercase();
        all_rows(&self.home)
            .into_iter()
            .filter(|r| {
                q.is_empty()
                    || r.name.to_lowercase().contains(&q)
                    || r.because.to_lowercase().contains(&q)
                    || r.address.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub(crate) fn select(&mut self, name: &str, cx: &mut Context<Self>) {
        self.selected = Some(name.to_owned());
        if let Some(st) = &self.stack
            && st.running
        {
            self.pane = pane(st, name, self.hub.now());
        }
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let names: Vec<String> = self.shown().iter().map(|r| r.name.clone()).collect();
        let i = self
            .selected
            .as_ref()
            .and_then(|s| names.iter().position(|n| n == s));
        let j = match i {
            Some(i) => i
                .saturating_add_signed(by)
                .min(names.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(n) = names.get(j) {
            self.select(&n.clone(), cx);
        }
    }

    fn say(&mut self, words: &str, cx: &mut Context<Self>) {
        cx.emit(OverviewEvent::Say(words.to_owned()));
    }

    fn copy_address(&mut self, cx: &mut Context<Self>) {
        let row = self.selected.as_ref().and_then(|s| {
            all_rows(&self.home)
                .into_iter()
                .find(|r| &r.name == s)
                .cloned()
        });
        match row {
            Some(r) if !r.address.is_empty() => {
                let a = if r.url.is_empty() {
                    r.address.clone()
                } else {
                    r.url.clone()
                };
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(a.clone()));
                self.say(&format!("Copied {a}"), cx);
            }
            Some(r) => self.say(&format!("{} has no address to copy.", r.name), cx),
            None => {}
        }
    }

    fn arrow(&mut self, dx: isize, dy: isize, cx: &mut Context<Self>) {
        match self.view {
            View::Grid => grid::step(self, dx, dy, cx),
            View::Tiles if dx != 0 => self.step(dx, cx),
            View::Tiles => self.step(dy * tiles::per_row(self) as isize, cx),
            _ if dy != 0 => self.step(dy, cx),
            _ => {}
        }
    }
}

/// Every row, group by group.
pub(crate) fn all_rows(h: &Home) -> Vec<&Row> {
    h.groups.iter().flat_map(|g| g.rows.iter()).collect()
}

/// doze.hcl saved after the running stack read it (Go `ConfigChangedOf`).
fn changed(st: &StackView, now: SystemTime) -> Option<doze_desk::overview::ConfigChanged> {
    let mtime = std::fs::metadata(Path::new(&st.dir).join("doze.hcl"))
        .and_then(|m| m.modified())
        .ok()?;
    doze_desk::overview::config_changed(mtime, st.loaded, now)
}

/// What the stack's verbs say in M1.
pub(crate) const M2: &str =
    "Waking, sleeping and starting come with the next step; this build looks, it does not touch.";

impl Render for OverviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let width = f32::from(window.viewport_size().width);
        self.width = width;
        let body = match self.view {
            View::Tiles => tiles::view(&t, self, cx).into_any_element(),
            View::List => list::view(&t, self, cx).into_any_element(),
            View::Map => div()
                .flex_1()
                .min_h_0()
                .mx(px(28.))
                .mb(px(14.))
                .child(self.map.clone())
                .into_any_element(),
            View::Grid => grid::view(&t, self, cx).into_any_element(),
        };
        let lit = self.map.read(cx).lit.is_some();
        div()
            .key_context("Overview")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .on_action(cx.listener(|this, _: &ViewTiles, _, cx| this.set_view(View::Tiles, cx)))
            .on_action(cx.listener(|this, _: &ViewList, _, cx| this.set_view(View::List, cx)))
            .on_action(cx.listener(|this, _: &ViewMap, _, cx| this.set_view(View::Map, cx)))
            .on_action(cx.listener(|this, _: &ViewGrid, _, cx| this.set_view(View::Grid, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.arrow(0, -1, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.arrow(0, 1, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.arrow(-1, 0, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.arrow(1, 0, cx)))
            .on_action(cx.listener(|this, _: &CopyAddress, _, cx| this.copy_address(cx)))
            .on_action(cx.listener(|this, _: &ToggleService, _, cx| this.say(M2, cx)))
            .on_action(cx.listener(|this, _: &OpenService, _, cx| {
                this.say(
                    "A service's own page comes with the instance page (M1 step 1.5).",
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &FilterRows, window, cx| {
                let f = this.filter.clone();
                f.update(cx, |i, cx| i.focus(window, cx));
            }))
            .child(head::head(&t, self, cx))
            .children(head::banners(&t, self))
            .child(strip::strip(&t, self, width))
            .child(body)
            .child(keys::keys(&t, self, lit))
    }
}
