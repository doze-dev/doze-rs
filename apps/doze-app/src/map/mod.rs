//! The Map: who calls whom, laid out by doze-map and painted here. A camera over the drawing (pinch or ⌘-scroll to
//! zoom about the pointer, scroll or drag to pan, fit, 1:1), only what is in view built, and levels of detail:
//!
//! - from [`DETAIL_ZOOM`] up, cards as the Go map drew them, every call a curve with its head;
//! - from [`LABEL_ZOOM`], named blocks, calls as polylines;
//! - below it, plain blocks; and in a large map, the calls between regions bundled into weighted lines
//!   ([`doze_map::Layout::overview`]) instead of thousands of single ones.
//!
//! A card's own calls light up on hover and stay lit on selection, at every level. The zoom toolbar sits above the
//! drawing, never over it; the minimap moves out of the way of the card in hand and can be put away.

mod cache;
mod camera;
mod cards;
mod chrome;
mod detail;
mod paint;
mod view;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use doze_map::{CardId, Layout, Mapper, Options, Overview};
use gpui::{AppContext as _, Context, Entity, FocusHandle, Window};
use gpui_component::input::{InputEvent, InputState};

use crate::model::{Fold, Page, Row, state_index};
use cache::Cache;
pub(crate) use camera::Camera;

/// Below this zoom cards are blocks without text and calls are polylines.
pub(crate) const DETAIL_ZOOM: f32 = 0.42;
/// From this zoom up to [`DETAIL_ZOOM`], blocks carry their names; below it, a large map shows bundles.
pub(crate) const LABEL_ZOOM: f32 = 0.2;

/// The Map of one stack.
pub(crate) struct MapView {
    mapper: Mapper,
    pub(crate) layout: Arc<Layout>,
    /// The layout's overview, made once per layout.
    pub(crate) overview: Rc<Overview>,
    pub(crate) rows: HashMap<String, Row>,
    pub(crate) folds: HashMap<String, Fold>,
    pub(crate) cam: Camera,
    /// The selected card: its lines drawn, the rest faded, its detail shown.
    pub(crate) lit: Option<CardId>,
    /// The card under the pointer: its lines drawn.
    pub(crate) hover: Option<CardId>,
    /// The minimap is wanted (it shows only when the drawing is well past the viewport).
    pub(crate) minimap: bool,
    pub(crate) search: Option<Entity<InputState>>,
    pub(crate) hits: Vec<CardId>,
    /// The highlighted hit.
    pub(crate) hit: usize,
    /// The drawing's place in the window and its size, as last painted.
    pub(crate) viewport: Rc<std::cell::Cell<(f32, f32, f32, f32)>>,
    pub(crate) cache: Rc<Cache>,
    /// Each card's state ([`state_index`]), for painting blocks without a lookup.
    pub(crate) states: Rc<Vec<u8>>,
    drag: Option<view::Drag>,
    pub(crate) focus: FocusHandle,
}

impl MapView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let mut mapper = Mapper::new(Options::default());
        let layout = mapper.layout(&[]);
        MapView {
            mapper,
            overview: Rc::new(layout.overview()),
            layout,
            rows: HashMap::new(),
            folds: HashMap::new(),
            cam: Camera::default(),
            lit: None,
            hover: None,
            minimap: true,
            search: None,
            hits: Vec::new(),
            hit: 0,
            viewport: Rc::default(),
            cache: Rc::new(Cache::default()),
            states: Rc::new(Vec::new()),
            drag: None,
            focus: cx.focus_handle(),
        }
    }

    /// A new page: laid out again only when the stack's shape changed (a change of state moves nothing and costs a
    /// hash), redrawn only when something the Map shows changed.
    pub(crate) fn set_page(&mut self, page: &Page, cx: &mut Context<Self>) {
        let l = self.mapper.layout(&page.map);
        let mut changed = false;
        if !Arc::ptr_eq(&l, &self.layout) {
            let lit = self
                .lit
                .and_then(|c| self.layout.card(c))
                .map(|c| c.name.clone());
            self.overview = Rc::new(l.overview());
            self.layout = l;
            self.cache.clear();
            self.lit = lit.and_then(|n| self.layout.card_named(&n));
            self.hover = None;
            if self.layout.cards.is_empty() {
                self.cam.fitted = false;
            }
            changed = true;
        }
        let rows: HashMap<String, Row> = page
            .rows
            .iter()
            .map(|r| (r.name.clone(), r.clone()))
            .collect();
        if rows != self.rows || page.folds != self.folds {
            self.rows = rows;
            self.folds = page.folds.clone();
            changed = true;
        }
        let states: Vec<u8> = self
            .layout
            .cards
            .iter()
            .map(|c| state_index(self.rows.get(&c.name)))
            .collect();
        if *self.states != states {
            self.states = Rc::new(states);
            self.cache.clear();
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// The card called `name`, if the map has it.
    pub(crate) fn card_named(&self, name: &str) -> Option<CardId> {
        self.layout.card_named(name)
    }

    /// Selects a card (or nothing), bringing it into view.
    pub(crate) fn select(&mut self, id: Option<CardId>, cx: &mut Context<Self>) {
        if let Some(id) = id {
            self.cam.reveal(&self.layout, id);
        }
        self.lit = id;
        cx.notify();
    }

    /// Opens the find box (or focuses it again).
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Jump to a card"));
            cx.subscribe(&input, |this: &mut MapView, input, ev: &InputEvent, cx| {
                match ev {
                    InputEvent::Change => {
                        let q = input.read(cx).value().to_string();
                        this.hits = this.layout.search(&q).into_iter().take(8).collect();
                        this.hit = 0;
                    }
                    InputEvent::PressEnter { .. } => {
                        if let Some(&id) = this.hits.first() {
                            this.jump_to(id, cx);
                        }
                    }
                    _ => {}
                }
                cx.notify();
            })
            .detach();
            self.search = Some(input);
        }
        if let Some(s) = &self.search {
            s.update(cx, |i, cx| i.focus(window, cx));
        }
        cx.notify();
    }

    /// Closes what is open, innermost first: the find box, then the selection. Says whether it closed anything.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.search.take().is_some() {
            self.hits.clear();
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        if self.lit.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// Centres a card, close enough to read, and selects it.
    pub(crate) fn jump_to(&mut self, id: CardId, cx: &mut Context<Self>) {
        self.cam.jump(&self.layout, id);
        self.search = None;
        self.hits.clear();
        self.lit = Some(id);
        cx.notify();
    }

    /// The arrow keys: the nearest card that way.
    pub(crate) fn step(&mut self, dx: f64, dy: f64, cx: &mut Context<Self>) {
        if let Some(next) = camera::next_card(&self.layout, &self.cam, self.lit, dx, dy) {
            self.select(Some(next), cx);
        }
    }

    /// The card whose lines are drawn: the selection, else the one under the pointer.
    pub(crate) fn focus_card(&self) -> Option<CardId> {
        self.lit.or(self.hover)
    }
}
