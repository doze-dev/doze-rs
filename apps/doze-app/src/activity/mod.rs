//! Activity (PLAN.md §2.8 row 80, M3 step 3.3): what a stack did over the last hour, day or week, from its journal on
//! disk. `doze_desk::activity` works out every figure; this draws them, as Go's page did: the summary (asleep while up,
//! memory left free, wakes, crashes, up), Trouble, each service's strip, What happened.
//!
//! Past Go:
//! - The page is one virtualised list (gpui's `list`), so a 2,000-service stack scrolls at the frame rate: only the
//!   rows in view are built, and each strip is painted on a canvas, not built of elements.
//! - A strip answers the pointer at once: the stretch under it lights, and a card says when it woke and slept, why,
//!   and how long the start took.
//! - The keyboard: `j`/`k` walk the services, ⏎ shows one alone (again for every service), `/` finds one by name,
//!   `[` and `]` step the span.
//! - A span switch keeps the page as it was until the new figures are ready, then the strips fade over: no flash of
//!   "reading the journal…".
//! - It reads again every ten seconds while shown, and when the journal moves, off the UI thread; it redraws only
//!   when a figure changed.

mod head;
mod rows;
mod strip;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use doze_desk::activity::{Activity, Ask, Span, activity_of, events_of};
use doze_desk::{Batch, Hub, Parts, StackKey};
use gpui::{
    AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ListAlignment, ListState, Pixels, Point, Render, Window, div, prelude::*, px,
};
use gpui_component::input::{InputEvent, InputState};

use crate::actions::{
    CloseOverlay, Confirm, FilterRows, MoveDown, MoveUp, SpanLonger, SpanShorter,
};

/// How often the page reads the journal again while it shows.
const EVERY: Duration = Duration::from_secs(10);

/// What the page tells the window.
#[derive(Debug, Clone)]
pub(crate) enum ActivityEvent {
    /// Back to the stack's page.
    Back,
}

/// What the pointer is over: a stretch or a crash on a row's strip.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hover {
    pub(crate) row: usize,
    /// The stretch, or the mark when `mark`.
    pub(crate) at: usize,
    pub(crate) mark: bool,
    pub(crate) pos: Point<Pixels>,
}

/// Each service's memory when last seen awake, by stack and name, while the app runs (Go `rememberRAM`).
#[derive(Default)]
pub(crate) struct RamSeen(pub(crate) HashMap<(String, String), i64>);

impl gpui::Global for RamSeen {}

/// The Activity page of one stack.
pub(crate) struct ActivityView {
    hub: Hub,
    pub(crate) key: StackKey,
    pub(crate) span: Span,
    /// One service alone, or empty.
    pub(crate) only: String,
    /// The names the find box keeps, or empty.
    pub(crate) find: String,
    pub(crate) model: Option<Activity>,
    /// The rows shown: the model's, less those the find box leaves out (indices into `model.rows`).
    pub(crate) shown: Vec<usize>,
    pub(crate) list: ListState,
    pub(crate) cur: Option<usize>,
    pub(crate) hover: Option<Hover>,
    /// Each drawn strip's bounds, by shown row, for the pointer.
    pub(crate) tracks: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    pub(crate) input: Option<Entity<InputState>>,
    /// The read in flight, by number; and how many reads have landed (the strips fade in on a span switch).
    asked: u64,
    pub(crate) landed: u64,
    /// Trouble shows every line.
    pub(crate) all_trouble: bool,
    pub(crate) focus: FocusHandle,
}

impl EventEmitter<ActivityEvent> for ActivityView {}

impl ActivityView {
    pub(crate) fn new(hub: Hub, key: StackKey, cx: &mut Context<Self>) -> Self {
        let mut v = ActivityView {
            hub,
            key,
            span: Span::Day,
            only: String::new(),
            find: String::new(),
            model: None,
            shown: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            cur: None,
            hover: None,
            tracks: Rc::default(),
            input: None,
            asked: 0,
            landed: 0,
            all_trouble: false,
            focus: cx.focus_handle(),
        };
        v.read(cx);
        // Read again every ten seconds while the page lives (it is dropped when the window leaves it).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(EVERY).await;
                if this.update(cx, |v, cx| v.read(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        v
    }

    /// Reads the journal and lays it out, off the UI thread; the page keeps what it shows until the answer lands.
    pub(crate) fn read(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(&self.key) else {
            return;
        };
        let ram = remember_ram(&st, cx);
        self.asked += 1;
        let (n, span, only, now) = (self.asked, self.span, self.only.clone(), self.hub.now());
        let zone = crate::instance::zone();
        let job = cx.background_executor().spawn(async move {
            let events = events_of(&st);
            let ask = Ask {
                span,
                inst: &only,
                ram: &ram,
                now,
                zone,
            };
            activity_of(&st, events, ask)
        });
        cx.spawn(async move |this, cx| {
            let a = job.await;
            let _ = this.update(cx, |v, cx| {
                if v.asked == n {
                    v.landed_model(a, cx);
                }
            });
        })
        .detach();
    }

    fn landed_model(&mut self, a: Activity, cx: &mut Context<Self>) {
        let span_moved = self
            .model
            .as_ref()
            .is_some_and(|m| m.span != a.span || m.inst != a.inst);
        if self.model.as_ref() == Some(&a) {
            return;
        }
        if span_moved {
            self.landed += 1;
        }
        self.model = Some(a);
        self.refilter(cx);
    }

    /// The rows the find box keeps; the list told how many items it has now.
    pub(crate) fn refilter(&mut self, cx: &mut Context<Self>) {
        let find = self.find.to_lowercase();
        self.shown = self
            .model
            .as_ref()
            .map(|m| {
                m.rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| find.is_empty() || r.name.to_lowercase().contains(&find))
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        let old = self.list.item_count();
        self.list.splice(0..old, rows::items(self.shown.len()));
        self.cur = self.cur.filter(|c| *c < self.shown.len());
        self.hover = None;
        cx.notify();
    }

    /// A batch from the hub: the journal moved, so read again (the ten-second read covers the rest).
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        if batch.parts(&self.key).intersects(Parts::JOURNAL) {
            self.read(cx);
        }
    }

    pub(crate) fn set_span(&mut self, span: Span, cx: &mut Context<Self>) {
        if self.span != span {
            self.span = span;
            self.read(cx);
            cx.notify();
        }
    }

    /// One service alone, or (the same again, or empty) every service.
    pub(crate) fn set_only(&mut self, name: &str, cx: &mut Context<Self>) {
        self.only = if self.only == name {
            String::new()
        } else {
            name.to_owned()
        };
        self.cur = None;
        self.read(cx);
        cx.notify();
    }

    fn step_span(&mut self, longer: bool, cx: &mut Context<Self>) {
        let i = Span::ALL.iter().position(|s| *s == self.span).unwrap_or(1);
        let j = if longer {
            (i + 1).min(2)
        } else {
            i.saturating_sub(1)
        };
        if let Some(s) = Span::ALL.get(j) {
            self.set_span(*s, cx);
        }
    }

    fn step_row(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.shown.len();
        if n == 0 {
            return;
        }
        let next = match self.cur {
            None => 0,
            Some(c) => c.saturating_add_signed(by).min(n - 1),
        };
        self.cur = Some(next);
        self.list.scroll_to_reveal_item(rows::FIRST_ROW + next);
        cx.notify();
    }

    /// Esc lets go of what the page holds (the find box, one service alone), then goes back to the stack.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find.is_empty() || self.input.is_some() {
            self.find.clear();
            self.input = None;
            self.refilter(cx);
            window.focus(&self.focus, cx);
        } else if !self.only.is_empty() {
            self.set_only("", cx);
        } else {
            cx.emit(ActivityEvent::Back);
        }
    }

    /// The find box, opened by `/`.
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match &self.input {
            Some(i) => i.clone(),
            None => {
                let i = cx.new(|cx| InputState::new(window, cx).placeholder("Find a service"));
                cx.subscribe_in(
                    &i,
                    window,
                    |v: &mut ActivityView, i, ev: &InputEvent, window, cx| match ev {
                        InputEvent::Change => {
                            v.find = i.read(cx).value().to_string();
                            v.refilter(cx);
                        }
                        InputEvent::PressEnter { .. } => window.focus(&v.focus, cx),
                        _ => {}
                    },
                )
                .detach();
                self.input = Some(i.clone());
                i
            }
        };
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    /// The find box opened with `words` in it (a shot, a test).
    #[cfg_attr(not(feature = "capture"), allow(dead_code))] // WHY: only shots and tests type into it from outside
    pub(crate) fn find_words(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
        if let Some(i) = self.input.clone() {
            i.update(cx, |i, cx| i.set_value(words.to_owned(), window, cx));
        }
        self.find = words.to_owned();
        self.refilter(cx);
    }

    /// The pointer moved over a strip: what is under it, if anything.
    pub(crate) fn point(&mut self, row: usize, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let b = self.tracks.borrow().get(&row).copied();
        let hit = b.and_then(|b| strip::hit(self, row, b, pos));
        let hover = hit.map(|(at, mark)| Hover { row, at, mark, pos });
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    pub(crate) fn unpoint(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.hover.as_ref().is_some_and(|h| h.row == row) {
            self.hover = None;
            cx.notify();
        }
    }
}

/// Keeps each awake service's memory, so a slept hour can be priced after the service has gone to sleep; and gives
/// what is known for this stack.
fn remember_ram(st: &doze_desk::StackView, cx: &mut Context<ActivityView>) -> HashMap<String, i64> {
    let key = st.key.dir().to_string_lossy().into_owned();
    if !cx.has_global::<RamSeen>() {
        cx.set_global(RamSeen::default());
    }
    let seen = cx.global_mut::<RamSeen>();
    for v in st.instances.iter().filter(|v| v.ram > 0) {
        seen.0.insert((key.clone(), v.name.clone()), v.ram);
    }
    seen.0
        .iter()
        .filter(|((k, _), _)| *k == key)
        .map(|((_, n), r)| (n.clone(), *r))
        .collect()
}

impl Focusable for ActivityView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ActivityView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::tokens(cx);
        let page = div()
            .id("activity")
            .key_context("Activity")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .on_action(cx.listener(|v, _: &SpanShorter, _, cx| v.step_span(false, cx)))
            .on_action(cx.listener(|v, _: &SpanLonger, _, cx| v.step_span(true, cx)))
            .on_action(cx.listener(|v, _: &MoveDown, _, cx| v.step_row(1, cx)))
            .on_action(cx.listener(|v, _: &MoveUp, _, cx| v.step_row(-1, cx)))
            .on_action(cx.listener(|v, _: &FilterRows, window, cx| v.open_find(window, cx)))
            .on_action(cx.listener(|v, _: &CloseOverlay, window, cx| v.escape(window, cx)))
            .on_action(cx.listener(|v, _: &Confirm, _, cx| {
                let name = v.cur.and_then(|c| v.row_at(c)).map(|r| r.name.clone());
                if let Some(n) = name {
                    v.set_only(&n, cx);
                }
            }))
            .child(head::head(&t, self, cx));
        let Some(_) = &self.model else {
            return page.child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(t.dim)
                    .child("reading the journal…"),
            );
        };
        page.child(rows::list(&t, self, cx))
            .children(strip::hover_card(&t, self))
            .child(crate::kit::keys_bar(
                &t,
                &[
                    ("[ ]".to_owned(), "span"),
                    (crate::kit::chip_of("Find a service"), "find"),
                    (crate::kit::chip_of("That service alone"), "alone"),
                    ("esc".into(), "back to the stack"),
                ],
                "from a stack: Activity in its head, or A; from ⌘K: activity",
            ))
    }
}

impl ActivityView {
    /// The shown row `i`.
    pub(crate) fn row_at(&self, i: usize) -> Option<&doze_desk::activity::Row> {
        let m = self.model.as_ref()?;
        m.rows.get(*self.shown.get(i)?)
    }
}
