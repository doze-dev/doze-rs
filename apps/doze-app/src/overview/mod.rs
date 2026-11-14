//! A stack's page (the Tiles, List, Map and Grid boards; Go `pages/overview.tsx`): the head with the view switch and
//! the stack's switch, one strip of figures, then the stack in the view remembered for it. A stopped stack is the same
//! page in another state: the same rows in the same places, each down, the strip saying what Up starts.
//!
//! Every word is doze-desk's ([`doze_desk::overview`]); this keeps the selection, the filter, the Grid's crosshair.
//! The page watches the stack's meters while it shows, so its figures are the last second's; it redraws only when what
//! it shows changed.

mod act;
mod addr;
mod banners;
mod choose;
mod foot;
mod grid;
mod head;
mod keys;
mod kv;
mod list;
mod pane;
mod strip;
mod tiles;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use doze_control::Topic;
use doze_desk::overview::{Home, Pane, Refs, Row, home, pane, refs_in, stopped_home};
use doze_desk::{Hub, StackKey, StackView, Watch};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    Render, Window, div, prelude::*, px,
};
use gpui_component::input::{InputEvent, InputState};

pub(crate) use banners::{broken_banner, stale_banner};

use crate::actions::{
    ChooseStart, CloseOverlay, CopyAddress, DismissFailure, FilterRows, MoveDown, MoveLeft,
    MoveRight, MoveUp, OpenRowService, OpenService, PinIt, RestartIt, SleepIt, ToggleService,
    TryAgain, UpAsLastTime, ViewGrid, ViewList, ViewMap, ViewTiles, WakeIt,
};
use crate::facts::Facts;
use crate::map::MapView;
use crate::model::{self, Page};
use crate::theme::{Density, tokens};
use doze_control::Verb;

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
    /// Open the page of the instance named, on its tab (`None`: the tab it opens on).
    Open(String, Option<&'static str>),
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
    /// Each shown service's name as a control the keyboard reaches by Tab (Go `Name`), and the one with the keys.
    pub(super) names: (HashMap<String, gpui::FocusHandle>, Option<String>),
    pub(crate) pane: Option<Pane>,
    pub(crate) filter: Entity<InputState>,
    pub(crate) query: String,
    pub(crate) density: Density,
    /// The Grid's cell under the pointer or the keys: row, column.
    pub(crate) cur: Option<(usize, usize)>,
    /// Where the Grid is scrolled to.
    pub(crate) grid: grid::Scroll,
    refs: HashMap<String, Refs>,
    watch: Option<Watch>,
    /// The stack's logs, watched while a row is selected (Go: home + a selected row streams its logs), so its Logs
    /// open on lines already heard.
    pub(super) logs: Option<Watch>,
    /// The window's width at the last render (the Tiles go four, three or two across).
    pub(crate) width: f32,
    /// The failure banner's "What doze printed" is open.
    pub(crate) printed: bool,
    /// The head's ⋯ menu is open.
    pub(crate) menu: bool,
    /// The filter box has the keys (its frame takes the accent's tint).
    pub(crate) filter_on: bool,
    /// Where the Tiles are scrolled to: held here, so only the person (or a new stack) moves it, never a redraw.
    pub(crate) tiles_scroll: gpui::ScrollHandle,
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
        act::observe_acts(cx);
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
        let map = cx.new(MapView::new);
        cx.subscribe(
            &map,
            |this: &mut OverviewView, _, ev: &crate::map::MapEvent, cx| {
                if let crate::map::MapEvent::Select(name) = ev {
                    this.map_selected(name.as_deref(), cx);
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
            map,
            view: View::default(),
            selected: None,
            names: (HashMap::new(), None),
            pane: None,
            filter,
            query: String::new(),
            density: Density::parse(&crate::prefs::density()).unwrap_or_default(),
            cur: None,
            grid: grid::Scroll::default(),
            refs: HashMap::new(),
            watch: None,
            logs: None,
            width: 1280.,
            printed: false,
            menu: false,
            filter_on: false,
            tiles_scroll: gpui::ScrollHandle::new(),
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
        self.logs = None;
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
        self.menu = false;
        self.tiles_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.map.update(cx, |m, cx| m.select(None, cx));
        self.cur = None;
        (self.grid.x, self.grid.y) = (0., 0.);
        self.refresh(cx);
    }

    /// Stops watching the stack's meters (the page is not shown).
    pub(crate) fn hide(&mut self) {
        self.watch = None;
        self.logs = None;
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
        let (mut h, map_view) = if st.running {
            let mut h = home(&st, now);
            h.changed = banners::changed(&st, now);
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
        self.machine(&mut h, &st, cx);
        let rows = all_rows(&h);
        // Nothing is selected until a click or a key selects it (Go starts with none); a row that went away lets go.
        if self
            .selected
            .as_ref()
            .is_some_and(|s| !rows.iter().any(|r| &r.name == s))
        {
            self.selected = None;
        }
        let logs = self.selected.is_some() && st.running;
        if logs != self.logs.is_some() {
            self.logs = logs.then(|| self.hub.watch(&st.key, Topic::Logs));
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
        // The Map's selection starts clear on each view change (Go).
        if v != self.view {
            self.map.update(cx, |m, cx| m.select(None, cx));
        }
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

    pub(super) fn say(&mut self, words: &str, cx: &mut Context<Self>) {
        cx.emit(OverviewEvent::Say(words.to_owned()));
    }

    /// Opens the instance `name`'s page (on `tab`, else its first); a stopped stack's rows have none.
    pub(crate) fn open(&mut self, name: &str, tab: Option<&'static str>, cx: &mut Context<Self>) {
        if self.stack.as_ref().is_some_and(|s| s.running) {
            cx.emit(OverviewEvent::Open(name.to_owned(), tab));
        }
    }

    /// ⏎: in the Grid, what the cell names (Go's tip `open`); elsewhere the row selected.
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        let name = match (self.view, self.cur) {
            (View::Grid, Some((r, c))) => self
                .home
                .grid
                .rows
                .get(r)
                .and_then(|row| row.cells.get(c))
                .and_then(|cell| cell.tip.as_ref())
                .map(|tip| tip.open.clone())
                .filter(|o| !o.is_empty()),
            _ => self.selected.clone(),
        };
        if let Some(n) = name {
            self.open(&n, None, cx);
        }
    }

    /// `o` in the Grid: the row's own service.
    fn open_row(&mut self, cx: &mut Context<Self>) {
        let name = self
            .cur
            .and_then(|(r, _)| self.home.grid.rows.get(r))
            .map(|row| row.name.clone());
        if let Some(n) = name {
            self.open(&n, None, cx);
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

impl Render for OverviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let width = f32::from(window.viewport_size().width);
        self.width = width;
        self.filter_on = self.filter.read(cx).focus_handle(cx).is_focused(window);
        self.keep_names(window, cx);
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
            View::Grid => grid::view(&t, self, window, cx).into_any_element(),
        };
        let lit = self.map.read(cx).lit.is_some();
        let lens = self.map.read(cx).lens.is_some();
        let small = self.map.read(cx).small();
        div()
            .key_context("Overview")
            .track_focus(&self.focus)
            .on_key_down(choose::tab)
            .flex()
            .flex_col()
            .size_full()
            .when(self.menu, |d| {
                d.on_action(cx.listener(|this, _: &CloseOverlay, _, cx| {
                    this.menu = false;
                    cx.notify();
                }))
            })
            .on_action(cx.listener(|this, _: &ViewTiles, _, cx| this.set_view(View::Tiles, cx)))
            .on_action(cx.listener(|this, _: &ViewList, _, cx| this.set_view(View::List, cx)))
            .on_action(cx.listener(|this, _: &ViewMap, _, cx| this.set_view(View::Map, cx)))
            .on_action(cx.listener(|this, _: &ViewGrid, _, cx| this.set_view(View::Grid, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.arrow(0, -1, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.arrow(0, 1, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.arrow(-1, 0, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.arrow(1, 0, cx)))
            .on_action(cx.listener(|this, _: &CopyAddress, _, cx| this.copy_address(cx)))
            .on_action(cx.listener(|this, _: &ToggleService, _, cx| this.toggle_selected(cx)))
            .on_action(cx.listener(|this, _: &UpAsLastTime, _, cx| this.up_last(cx)))
            .on_action(cx.listener(|this, _: &ChooseStart, _, cx| this.choose_start(cx)))
            .on_action(cx.listener(|this, _: &WakeIt, _, cx| this.selected_verb(Verb::Wake, cx)))
            .on_action(cx.listener(|this, _: &SleepIt, _, cx| this.selected_verb(Verb::Sleep, cx)))
            .on_action(
                cx.listener(|this, _: &RestartIt, _, cx| this.selected_verb(Verb::Restart, cx)),
            )
            .on_action(cx.listener(|this, _: &PinIt, _, cx| this.selected_verb(Verb::Pin, cx)))
            .on_action(cx.listener(|this, _: &TryAgain, _, cx| this.try_again(cx)))
            .on_action(cx.listener(|this, _: &DismissFailure, _, cx| this.dismiss_failure(cx)))
            .on_action(cx.listener(|this, _: &OpenService, _, cx| this.open_selected(cx)))
            .on_action(cx.listener(|this, _: &OpenRowService, _, cx| this.open_row(cx)))
            .on_action(cx.listener(|this, _: &FilterRows, window, cx| {
                let f = this.filter.clone();
                f.update(cx, |i, cx| i.focus(window, cx));
            }))
            .children(banners::stale(&t, self))
            .child(head::head(&t, self, cx))
            .children(act::banners(&t, self, cx))
            .children(banners::banners(&t, self, cx))
            .child(strip::strip(&t, self, width))
            .child(body)
            .child(keys::keys(&t, self, (lit, lens, small)))
    }
}
