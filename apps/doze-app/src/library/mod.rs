//! The Library, the first tab (the Library boards; Go `pages/library.tsx`): the doze mark and name, the help strip
//! while it is useful, every stack with its figures and its switch, what other doze programs run here, the machine
//! line. Every word is doze-desk's ([`doze_desk::library`]); this keeps only what is selected and which menu is open.
//!
//! The switch, the menu and the keys act through `crate::act` (M2): a busy row says what it is doing.

mod act;
mod menu;
mod parts;
mod rows;

use std::path::Path;
use std::time::{Duration, Instant};

use doze_desk::library::{Hints, Library, Other, library, machine, others};
use doze_desk::{Hub, StackKey, StackView};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Window, div,
    prelude::*, px,
};

use crate::actions::{MoveDown, MoveUp, OpenStack, RemoveStack, ToggleStack};
use crate::facts::Facts;
use crate::kit::{chip_of, keys_bar};
use crate::theme::{Density, tokens};

/// How often the names other programs hold are read again.
const OTHERS_EVERY: Duration = Duration::from_secs(5);

/// What the Library asks of the window.
#[derive(Debug, Clone)]
pub(crate) enum LibraryEvent {
    /// Show this stack's page.
    Open(StackKey),
    /// Say this at the foot for a moment.
    Say(String),
}

/// The Library page.
pub(crate) struct LibraryView {
    hub: Hub,
    facts: Entity<Facts>,
    pub(crate) lib: Library,
    /// Each row's stack, in the rows' order.
    keys: Vec<StackKey>,
    /// The selected row, by key.
    pub(crate) selected: Option<String>,
    /// The row whose ⋯ menu is open, by key.
    pub(crate) menu: Option<String>,
    pub(crate) density: Density,
    hints: Hints,
    others: (Option<Instant>, Vec<Other>),
    pub(crate) focus: FocusHandle,
}

impl EventEmitter<LibraryEvent> for LibraryView {}

impl Focusable for LibraryView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LibraryView {
    pub(crate) fn new(hub: Hub, facts: Entity<Facts>, cx: &mut Context<Self>) -> LibraryView {
        cx.observe(&facts, |this, _, cx| this.refresh(cx)).detach();
        let (dismissed, connected) = crate::prefs::hints();
        let mut v = LibraryView {
            hub,
            facts,
            lib: Library::default(),
            keys: Vec::new(),
            selected: None,
            menu: None,
            density: Density::parse(&crate::prefs::density()).unwrap_or_default(),
            hints: Hints {
                dismissed,
                connected,
            },
            others: (None, Vec::new()),
            focus: cx.focus_handle(),
        };
        v.refresh(cx);
        v
    }

    /// Reads every stack again; redraws only if the page changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let stacks = self.hub.stacks();
        let home = self.hub.home().to_path_buf();
        let (shapes, doctor, unset) = self.facts.update(cx, |f, cx| {
            let shapes: Vec<_> = stacks
                .iter()
                .map(|s| if s.running { None } else { f.shape(&s.dir, cx) })
                .collect();
            // The stacks whose own domains are not set up here (Go: their https pills are not offered as links).
            let unset: Vec<String> = stacks
                .iter()
                .filter(|s| f.setup(s, &home, cx).is_some())
                .map(|s| s.key.dir().display().to_string())
                .collect();
            (shapes, f.doctor(&stacks, cx), unset)
        });
        let shape_of = |st: &StackView| {
            stacks
                .iter()
                .position(|s| s.key == st.key)
                .and_then(|i| shapes.get(i).cloned().flatten())
        };
        let mut lib = library(&stacks, &shape_of, self.hints, self.hub.now());
        doze_desk::library::unready(&mut lib, &unset);
        lib.machine = doctor.map(|d| machine(&d, &stacks)).unwrap_or_default();
        if self.others.0.is_none_or(|at| at.elapsed() > OTHERS_EVERY) {
            self.others = (Some(Instant::now()), others(self.hub.home()));
        }
        lib.others = self.others.1.clone();
        if !self.hints.connected
            && stacks
                .iter()
                .any(|s| s.instances.iter().any(|v| v.conns > 0))
        {
            // The third step is done the first time a connection is seen, and stays done.
            self.hints.connected = true;
            crate::prefs::save_hint("connected");
        }
        self.keys = stacks.into_iter().map(|s| s.key).collect();
        if self
            .selected
            .as_ref()
            .is_none_or(|k| !lib.stacks.iter().any(|s| &s.key == k))
        {
            self.selected = lib.stacks.first().map(|s| s.key.clone());
        }
        if lib != self.lib {
            self.lib = lib;
            cx.notify();
        }
    }

    fn index(&self) -> Option<usize> {
        let k = self.selected.as_ref()?;
        self.lib.stacks.iter().position(|s| &s.key == k)
    }

    pub(crate) fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.selected = Some(key.to_owned());
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.lib.stacks.len();
        let Some(i) = self.index() else { return };
        let j = i.saturating_add_signed(by).min(n.saturating_sub(1));
        if let Some(s) = self.lib.stacks.get(j) {
            self.selected = Some(s.key.clone());
            cx.notify();
        }
    }

    /// Opens the stack `key`'s page.
    pub(crate) fn open(&mut self, key: &str, cx: &mut Context<Self>) {
        let at = self.lib.stacks.iter().position(|s| s.key == key);
        if let Some(k) = at.and_then(|i| self.keys.get(i)) {
            cx.emit(LibraryEvent::Open(k.clone()));
        }
    }

    pub(crate) fn say(&mut self, words: &str, cx: &mut Context<Self>) {
        cx.emit(LibraryEvent::Say(words.to_owned()));
    }

    /// The help strip put away for good.
    pub(crate) fn dismiss_steps(&mut self, cx: &mut Context<Self>) {
        self.hints.dismissed = true;
        crate::prefs::save_hint("dismissed");
        self.refresh(cx);
    }

    pub(crate) fn set_density(&mut self, d: Density, cx: &mut Context<Self>) {
        self.density = d;
        crate::prefs::save_density(d.word());
        cx.notify();
    }

    /// Opens a stack's doze.hcl in the person's editor, or its folder in Finder.
    pub(crate) fn reveal(&mut self, dir: &str, file: bool, cx: &mut Context<Self>) {
        self.menu = None;
        let dir = Path::new(dir);
        if file {
            crate::outward::open_path(cx, &dir.join("doze.hcl"));
        } else {
            crate::outward::reveal(cx, dir);
        }
        cx.notify();
    }
}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let empty = self.lib.stacks.is_empty();
        let mut body = div()
            .id("library-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(px(16.));
        if !self.lib.steps.is_empty() {
            body = body.child(parts::steps(&t, &self.lib.steps, cx));
        }
        body = if empty {
            body.child(parts::empty(&t, cx))
        } else {
            body.child(rows::table(&t, self, cx))
                .child(parts::model(&t))
        };
        if !self.lib.others.is_empty() {
            body = body.child(parts::others(&t, &self.lib.others));
        }
        let keys = [
            (
                format!("{}{}", chip_of("Stack above"), chip_of("Stack below")),
                "move",
            ),
            (chip_of("Up or down"), "up / down"),
            (chip_of("Open the stack"), "open"),
            (chip_of("Open folder…"), "open folder"),
            (chip_of("Remove from doze…"), "remove"),
        ];
        div()
            .key_context("Library")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &OpenStack, _, cx| {
                if let Some(k) = this.selected.clone() {
                    this.open(&k, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleStack, _, cx| this.toggle_selected(cx)))
            .on_action(cx.listener(|this, _: &RemoveStack, _, cx| this.remove_selected(cx)))
            .child(parts::head(&t, self, cx))
            .child(body)
            .child(parts::machine_line(&t, &self.lib.machine, cx))
            .child(keys_bar(
                &t,
                &keys,
                "Home is the first tab; it never closes",
            ))
    }
}
