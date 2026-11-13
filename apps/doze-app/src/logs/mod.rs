//! Logs (PLAN.md §2.5), one view in three places: an instance's Logs tab, All logs (every instance merged, with
//! doze's own events), and a task's output. The lines are the stack's whole ring (100,000), never copied: the page
//! keeps a [`LogIndex`] into the ring, takes in what arrives each frame, and draws only the rows in view, read from
//! the ring as they are drawn. A new filter is one pass over the ring, done off the UI thread, so typing never waits
//! for it; the old lines stay until the new ones are ready.
//!
//! Past Go: a filter can be a pattern (`/timeout|refused/`), find mode keeps every line and steps through the
//! matches (`f`, then `n` / `N`), a click picks a line and a shift-click a run of them for `c` to copy.

mod act;
mod list;
mod toolbar;

use std::collections::BTreeSet;
use std::ops::Range;
use std::time::SystemTime;

use doze_control::Topic;
use doze_desk::page::filter;
use doze_desk::page::logindex::{LogIndex, LogQuery};
use doze_desk::page::time::Zone;
use doze_desk::{Batch, Hub, Parts, StackKey, Watch};
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, Render,
    UniformListScrollHandle, Window, div, prelude::*,
};
use gpui_component::input::{InputEvent, InputState};

use crate::instance::zone;

/// How often streaming lines are taken in.
const PACE: std::time::Duration = std::time::Duration::from_millis(250);

/// Where the view is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogsMode {
    /// An instance's Logs tab: the toolbar, the lines, the keys.
    Tab,
    /// All logs: a head of its own too.
    All,
    /// A task's output: the lines only, of one run.
    Bare,
}

/// A logs view.
pub(crate) struct LogsView {
    pub(crate) hub: Hub,
    pub(crate) key: StackKey,
    pub(crate) instance: String,
    pub(crate) mode: LogsMode,
    pub(crate) index: LogIndex,
    /// The filter pass in flight, by number; a newer one makes an older one's answer moot.
    building: u64,
    pub(crate) level: u8,
    pub(crate) filter_text: String,
    pub(crate) find: bool,
    /// Which run: 0 every run, k the k-th from the newest.
    pub(crate) run_back: usize,
    /// The run a task's output is held to.
    pinned_run: u32,
    pub(crate) clock: bool,
    pub(crate) follow: bool,
    pub(crate) show: Option<BTreeSet<String>>,
    pub(crate) input: Option<Entity<InputState>>,
    pub(crate) scroll: UniformListScrollHandle,
    /// The hit stepped to, and its place among the hits.
    pub(crate) hit: Option<(usize, usize)>,
    /// The picked lines, as anchors (first, last), for copying.
    pub(crate) picked: Option<(u64, u64)>,
    /// The rows drawn last, for copying what is shown.
    pub(crate) drawn: Range<usize>,
    pub(crate) now: SystemTime,
    pub(crate) zone: Zone,
    pub(crate) focus: FocusHandle,
    /// When lines were last taken in, and whether a catch-up is waiting for its turn.
    taken: std::time::Instant,
    pending: bool,
    _watch: Watch,
}

impl LogsView {
    pub(crate) fn new(
        hub: Hub,
        key: StackKey,
        instance: &str,
        mode: LogsMode,
        cx: &mut Context<Self>,
    ) -> Self {
        let watch = hub.watch(&key, Topic::Logs);
        let now = hub.now();
        let mut v = LogsView {
            hub,
            key,
            instance: instance.to_owned(),
            mode,
            index: LogIndex::new(LogQuery::default()),
            building: 0,
            level: if mode == LogsMode::Bare { 0 } else { 1 },
            filter_text: String::new(),
            find: false,
            run_back: 0,
            pinned_run: 0,
            clock: false,
            follow: true,
            show: None,
            input: None,
            scroll: UniformListScrollHandle::new(),
            hit: None,
            picked: None,
            drawn: 0..0,
            now,
            zone: zone(),
            focus: cx.focus_handle(),
            taken: std::time::Instant::now(),
            pending: false,
            _watch: watch,
        };
        v.rebuild(cx);
        v
    }

    /// What the page asks the index for now.
    fn query(&self) -> LogQuery {
        let runs = self.index.runs();
        let run = if self.pinned_run > 0 {
            self.pinned_run
        } else if self.run_back > 0 {
            runs.len()
                .checked_sub(self.run_back)
                .and_then(|i| runs.get(i))
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        LogQuery {
            instance: self.instance.clone(),
            show: if self.instance.is_empty() {
                self.show.clone()
            } else {
                None
            },
            level: self.level,
            filter: filter::parse(&self.filter_text),
            run,
            find: self.find,
        }
    }

    /// Starts again for a new question: one pass over the ring, off the UI thread when the ring is big.
    pub(crate) fn rebuild(&mut self, cx: &mut Context<Self>) {
        let q = self.query();
        self.building += 1;
        self.hit = None;
        self.picked = None;
        let (hub, key, n) = (self.hub.clone(), self.key.clone(), self.building);
        let events = hub.stack(&key).map(|s| s.events).unwrap_or_default();
        let small = hub.with_logs(&key, |r| r.len() < 5_000).unwrap_or(true);
        let pass = move || {
            let mut idx = LogIndex::new(q);
            hub.with_logs(&key, |r| idx.update(r, &events));
            idx
        };
        if small {
            self.index = pass();
            self.settle(cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            let idx = cx.background_executor().spawn(async move { pass() }).await;
            let _ = this.update(cx, |v, cx| {
                if v.building == n {
                    v.index = idx;
                    v.catch_up(cx);
                    v.settle(cx);
                }
            });
        })
        .detach();
    }

    /// After a rebuild: the end in view when following, and a redraw.
    fn settle(&mut self, cx: &mut Context<Self>) {
        self.now = self.hub.now();
        if self.follow {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// Takes in what arrived since the index last looked.
    fn catch_up(&mut self, _cx: &mut Context<Self>) -> bool {
        let events = self
            .hub
            .stack(&self.key)
            .map(|s| s.events)
            .unwrap_or_default();
        let idx = &mut self.index;
        self.hub
            .with_logs(&self.key, |r| idx.update(r, &events))
            .unwrap_or(false)
    }

    /// A batch from the hub: new lines (or doze's events) taken in, the end kept in view while following.
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        if !batch
            .parts(&self.key)
            .intersects(Parts::LOGS | Parts::JOURNAL)
        {
            return;
        }
        // Lines that stream are taken in at most four times a second (Go paced its pushes at 250 ms): every redraw
        // holds the window at the compositor's floor, so a page that redrew each frame while following would sit
        // far over the memory budget. The first lines after a quiet spell show at once.
        if self.pending {
            return;
        }
        let wait = PACE.saturating_sub(self.taken.elapsed());
        if wait.is_zero() {
            self.take(cx);
            return;
        }
        self.pending = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |v, cx| {
                v.pending = false;
                v.take(cx);
            });
        })
        .detach();
    }

    fn take(&mut self, cx: &mut Context<Self>) {
        self.taken = std::time::Instant::now();
        if self.catch_up(cx) {
            self.settle(cx);
        }
    }

    /// Holds a task's output to one run (0: its newest).
    pub(crate) fn pin_run(&mut self, run: u32, cx: &mut Context<Self>) {
        if run != self.pinned_run || self.index.is_empty() {
            self.pinned_run = run;
            self.rebuild(cx);
        }
    }

    /// The merged page's Show list changed.
    pub(crate) fn set_show(&mut self, show: Option<BTreeSet<String>>, cx: &mut Context<Self>) {
        if show != self.show {
            self.show = show;
            self.rebuild(cx);
        }
    }

    pub(super) fn set_level(&mut self, level: u8, cx: &mut Context<Self>) {
        if self.level != level {
            self.level = level;
            self.rebuild(cx);
        }
    }

    pub(crate) fn set_run_back(&mut self, back: usize, cx: &mut Context<Self>) {
        let back = back.min(self.index.runs().len());
        if self.run_back != back {
            self.run_back = back;
            self.rebuild(cx);
        }
    }

    pub(crate) fn set_follow(&mut self, follow: bool, cx: &mut Context<Self>) {
        self.follow = follow;
        if follow {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// The filter box, made on first draw (it needs the window).
    pub(super) fn input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(i) = &self.input {
            return i.clone();
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter text, or /pattern/"));
        cx.subscribe(
            &input,
            |this: &mut LogsView, input, ev: &InputEvent, cx| match ev {
                InputEvent::Change => {
                    let text = input.read(cx).value().to_string();
                    if text != this.filter_text {
                        this.filter_text = text;
                        this.rebuild(cx);
                    }
                }
                InputEvent::PressEnter { .. } if this.find => this.step_hit(false, cx),
                _ => {}
            },
        )
        .detach();
        self.input = Some(input.clone());
        input
    }
}

impl Focusable for LogsView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LogsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::tokens(cx);
        let input = (self.mode != LogsMode::Bare).then(|| self.input(window, cx));
        let root = div()
            .id("logs")
            .font_features(gpui::FontFeatures::disable_ligatures())
            .key_context("Logs")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0();
        let root = self.actions(root, cx);
        if self.mode == LogsMode::Bare {
            return root.child(list::list(&t, self, cx)).into_any_element();
        }
        root.when(self.mode == LogsMode::All, |d| {
            d.child(toolbar::head(&t, self, cx))
        })
        .children(input.map(|i| toolbar::toolbar(&t, self, i, cx)))
        .child(toolbar::dock(&t, self, cx))
        .child(toolbar::keys(&t, self))
        .into_any_element()
    }
}
