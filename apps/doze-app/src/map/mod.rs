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
//!
//! Past sixty cards (doze-map's `classic_max`; a smaller map is the Go map exactly), a map that splits into named
//! regions is drawn by region, each a labelled frame (its services, what is crashing, its memory); its calls are on
//! demand (faint at rest, the regions' bundles between frames, `L` for every line); the cards with no calls fold
//! into one tile that opens a searchable list ([`tile`]); find flies to the card ([`flight`]); and two focus views
//! ([`lens`]) show a card's neighbourhood (`H`) or only what is in trouble (`T`).

#[cfg(feature = "capture")]
pub(crate) mod bench;
mod cache;
mod camera;
mod cards;
mod chrome;
mod detail;
mod fade;
mod flight;
mod lens;
mod paint;
mod regions;
#[cfg(feature = "capture")]
mod stage;
#[cfg(all(test, feature = "capture"))]
mod tests;
mod tile;
mod tools;
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
pub(crate) use lens::Lens;

/// Below this zoom cards are blocks without text and calls are polylines.
pub(crate) const DETAIL_ZOOM: f32 = 0.42;
/// From this zoom up to [`DETAIL_ZOOM`], blocks carry their names; below it, a large map shows bundles.
pub(crate) const LABEL_ZOOM: f32 = 0.2;

/// What the Map asks of the window.
#[derive(Debug, Clone)]
pub(crate) enum MapEvent {
    /// Open a card's instance: ⏎ on the selected card, or a double click.
    Open(String),
}

impl gpui::EventEmitter<MapEvent> for MapView {}

/// The Map of one stack.
pub(crate) struct MapView {
    mapper: Mapper,
    /// The stack's layout; [`MapView::layout`] is it, or a focus view made from it.
    pub(crate) base: Arc<Layout>,
    /// The layout drawn.
    pub(crate) layout: Arc<Layout>,
    /// The focus view in force, and the camera from before it.
    pub(crate) lens: Option<Lens>,
    saved: Option<Camera>,
    /// The page's instances, for a focus view laid out on its own.
    ins: Vec<doze_map::Instance>,
    /// Every line drawn (`L`), rather than lines on demand.
    pub(crate) all_lines: bool,
    /// The folded tile's list, open.
    pub(crate) list: Option<tile::List>,
    /// The selection's and the pointer's fades in flight.
    pub(crate) fades: fade::Fades,
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
            base: layout.clone(),
            layout,
            lens: None,
            saved: None,
            ins: Vec::new(),
            all_lines: false,
            list: None,
            fades: fade::Fades::default(),
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
        let moved = !Arc::ptr_eq(&l, &self.base);
        if moved {
            self.base = l;
            self.ins.clone_from(&page.map);
            if self.lens.is_none() {
                self.show(self.base.clone());
            }
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
            let trouble = |r: &HashMap<String, Row>| {
                let mut v: Vec<&str> = r
                    .values()
                    .filter(|r| r.trouble)
                    .map(|r| r.name.as_str())
                    .collect();
                v.sort_unstable();
                v.join(",")
            };
            // Only trouble is made again when what is in trouble changes.
            let again = moved
                || matches!(self.lens, Some(Lens::Trouble { .. }))
                    && trouble(&rows) != trouble(&self.rows);
            self.rows = rows;
            self.folds = page.folds.clone();
            if again {
                self.relens();
            }
            changed = true;
        } else if moved {
            self.relens();
        }
        changed |= self.restate();
        if changed {
            cx.notify();
        }
    }

    /// Draws `l`: the selection and the pointer's card kept by name, the cache and the states made again.
    pub(crate) fn show(&mut self, l: Arc<Layout>) {
        let name =
            |id: Option<CardId>| id.and_then(|c| self.layout.card(c)).map(|c| c.name.clone());
        let (lit, hover) = (name(self.lit), name(self.hover));
        self.overview = Rc::new(l.overview());
        self.layout = l;
        self.cache.clear();
        self.lit = lit.and_then(|n| self.layout.card_named(&n));
        self.hover = hover.and_then(|n| self.layout.card_named(&n));
        self.fades.shown(self.lit, self.hover);
        self.restate();
    }

    /// Each drawn card's state, made again; says whether it changed.
    fn restate(&mut self) -> bool {
        let states: Vec<u8> = self
            .layout
            .cards
            .iter()
            .map(|c| state_index(self.rows.get(&c.name)))
            .collect();
        if *self.states == states {
            return false;
        }
        self.states = Rc::new(states);
        self.cache.clear();
        true
    }

    /// Lines on demand: past sixty cards, unless every line is asked for.
    pub(crate) fn quiet(&self) -> bool {
        self.layout.mode == doze_map::Mode::Layered && !self.all_lines
    }

    /// Something on the Map that esc closes: the find box, the list, a focus view, a selection.
    pub(crate) fn holds(&self) -> bool {
        self.lit.is_some() || self.search.is_some() || self.list.is_some() || self.lens.is_some()
    }

    /// The card called `name`, if the map has it.
    pub(crate) fn card_named(&self, name: &str) -> Option<CardId> {
        self.layout.card_named(name)
    }

    /// Selects a card (or nothing), bringing it into view.
    pub(crate) fn select(&mut self, id: Option<CardId>, cx: &mut Context<Self>) {
        if let Some(id) = id {
            // Out of view, the camera eases to it (the arrow keys walk off the edge).
            let mut to = self.cam;
            to.flight = None;
            to.reveal(&self.layout, id);
            if to.pan != self.cam.pan {
                let (x, y) = to.centre();
                let dur = crate::theme::Motion::of(cx).normal;
                self.cam.fly((to.zoom, x, y), self.layout.bounds, dur);
            }
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
        if self.list.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        if self.leave(cx) {
            return true;
        }
        if self.lit.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// Flies to a card, close enough to read, and selects it; a folded card opens the tile's list on it.
    pub(crate) fn jump_to(&mut self, id: CardId, cx: &mut Context<Self>) {
        self.search = None;
        self.hits.clear();
        self.lit = Some(id);
        if self.layout.folded(id) {
            self.list = None;
            cx.notify();
            return;
        }
        if let Some(c) = self.layout.card(id) {
            let ctr = c.rect.center();
            let zoom = if self.cam.zoom < 0.9 {
                1.0
            } else {
                self.cam.zoom
            };
            let dur = crate::theme::Motion::of(cx).slow;
            self.cam.fly((zoom, ctr.x, ctr.y), self.layout.bounds, dur);
        }
        cx.notify();
    }

    /// Flies to show a region's frame whole.
    pub(crate) fn open_region(&mut self, k: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.layout.regions.get(k) {
            let to = self.cam.framing(r.rect);
            let dur = crate::theme::Motion::of(cx).slow;
            self.cam.fly(to, self.layout.bounds, dur);
            cx.notify();
        }
    }

    /// Fits `cam` to the drawing: a focus view is shown whole, whatever its layout.
    pub(crate) fn fit_into(&self, cam: &mut Camera) {
        let small = self.layout.mode == doze_map::Mode::Classic && self.lens.is_none();
        cam.fit_as(&self.layout, small);
    }

    /// Fits the camera to the drawing, at once.
    pub(crate) fn refit(&mut self) {
        let mut c = self.cam;
        self.fit_into(&mut c);
        self.cam = c;
    }

    /// Flies back to the whole map.
    pub(crate) fn fly_fit(&mut self, cx: &mut Context<Self>) {
        let mut end = self.cam;
        self.fit_into(&mut end);
        let (x, y) = end.centre();
        let dur = crate::theme::Motion::of(cx).slow;
        self.cam.fly((end.zoom, x, y), self.layout.bounds, dur);
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
