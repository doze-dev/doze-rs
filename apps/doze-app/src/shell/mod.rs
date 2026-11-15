//! The window's root: the title bar (Home and a tab per stack), the page shown (the Library, a stack's page in one of
//! its four views, or a page inside a stack: an instance's page or All logs), a line at the foot for a moment, and
//! the overlays over it all. It reads the hub: a batch that touches what is shown re-reads it, and a page re-renders
//! only when what it shows changed (the views are `PartialEq`), which is what keeps an idle window at zero redraws.

mod acting;
pub(crate) mod attn;
mod clock;
mod history;
mod inside;
mod nav;
mod needs;
mod ops;
mod pages;
#[cfg(all(test, feature = "capture"))]
mod pages_tests;
mod pick;
mod tasks;
#[cfg(all(test, feature = "capture"))]
mod tests;
mod titlebar;
mod wire;

use std::path::PathBuf;

use doze_desk::{Batch, Hub, StackKey};
use gpui::{
    AppContext as _, Context, Entity, ExternalPaths, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Window, div, prelude::*, px,
};

use crate::actions::{
    AllLogs, CloseOverlay, CloseWindow, Confirm, CycleDensity, CycleMode, GoHome, MoveDown, MoveUp,
    NextPalette, Quit,
};
use crate::facts::{self, Facts};
use crate::library::{LibraryEvent, LibraryView};
use crate::map::{MapEvent, MapView};
use crate::overlays::Overlay;
use crate::overview::{OverviewEvent, OverviewView};
use crate::theme::{tokens, ty};
use crate::widgets::UI_FAMILY;

pub(crate) use nav::{Screen, Tab};

/// The root view.
pub(crate) struct Root {
    hub: Hub,
    pub(crate) screen: Screen,
    pub(crate) library: Entity<LibraryView>,
    pub(crate) overview: Entity<OverviewView>,
    /// The stack page's Map (the overview's own), for links and shots that select a card.
    pub(crate) map: Entity<MapView>,
    /// The title bar's stack tabs: every running stack, and the stopped ones opened this session.
    pub(crate) tabs: Vec<Tab>,
    opened: Vec<StackKey>,
    /// Running stacks whose tab was closed; a tab comes back when its stack is opened again.
    hidden: Vec<StackKey>,
    overlay: Option<Overlay>,
    /// The page inside a stack while [`Screen::Instance`] or [`Screen::Logs`] shows: the view the screen names.
    inside: Option<inside::Inside>,
    /// Acting: dialogs, sheets, toasts, stack actions (M2).
    pub(crate) acts: Entity<crate::act::Acts>,
    toasts: Entity<crate::overlays::toasts::ToastsView>,
    /// The page of a folder with no doze.hcl, while [`Screen::Empty`] shows.
    pub(crate) empty: Option<Entity<crate::empty::EmptyView>>,
    focus: FocusHandle,
    /// Needs you: the list, the alerts, what happened while the window was closed.
    pub(crate) attn: attn::Attn,
    /// The way back and forward.
    hist: history::History,
    /// Reads what is shown again while it counts down (see `clock`).
    clock: clock::Clock,
    /// Each task's last record, to say how a run went (see `tasks`).
    tasks: tasks::Tasks,
    /// Activity and Settings (M3).
    pub(crate) pages: pages::Pages,
}

impl Root {
    /// The window's root, on the stack `first` when there is one (a fixture's, a link's), else on the Library.
    pub(crate) fn new(
        hub: Hub,
        first: Option<StackKey>,
        mode: facts::Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let acts = crate::act::ensure(&hub, &mode, cx);
        let fixture = matches!(mode, facts::Mode::Fixture);
        let toasts = cx.new(|cx| crate::overlays::toasts::ToastsView::new(acts.clone(), cx));
        let facts = cx.new(|_| Facts::new(mode));
        let library = cx.new(|cx| LibraryView::new(hub.clone(), facts.clone(), cx));
        cx.observe(&facts, |this: &mut Root, _, cx| this.rebuild_attention(cx))
            .detach();
        cx.observe(&acts, |this: &mut Root, _, cx| this.acting_moved(cx))
            .detach();
        let attn = attn::Attn::new(facts.clone());
        let overview = cx.new(|cx| OverviewView::new(hub.clone(), facts.clone(), window, cx));
        cx.subscribe_in(
            &library,
            window,
            |this, _, ev: &LibraryEvent, window, cx| match ev {
                LibraryEvent::Open(k) => this.go(Screen::Stack(k.clone()), window, cx),
                LibraryEvent::Say(s) => this.say(s.clone(), cx),
            },
        )
        .detach();
        cx.subscribe_in(
            &overview,
            window,
            |this, _, ev: &OverviewEvent, window, cx| match ev {
                OverviewEvent::Say(s) => this.say(s.clone(), cx),
                OverviewEvent::Open(n, tab) => {
                    this.open_instance(n, window, cx);
                    this.open_tab(*tab, window, cx);
                }
            },
        )
        .detach();
        let map = overview.read(cx).map.clone();
        cx.subscribe_in(&map, window, |this, _, ev: &MapEvent, window, cx| {
            if let MapEvent::Open(name) = ev {
                this.open_instance(name, window, cx);
            }
        })
        .detach();
        let mut root = Root {
            hub,
            screen: Screen::Library,
            library,
            overview,
            map,
            tabs: Vec::new(),
            opened: Vec::new(),
            hidden: Vec::new(),
            overlay: None,
            inside: None,
            acts: acts.clone(),
            toasts,
            empty: None,
            focus: cx.focus_handle(),
            attn,
            hist: history::History::default(),
            clock: clock::Clock::default(),
            tasks: tasks::Tasks::default(),
            pages: pages::Pages::new(facts.clone(), fixture),
        };
        root.rebuild_attention(cx);
        root.hear(&acts, window, cx);
        // A fixture or a link names the stack; else the page the window last showed, when Settings › General's
        // "Reopen the page I had" is on (Go: reopen where I was).
        let screen = match first {
            Some(k) => Screen::Stack(k),
            None => root.reopened(),
        };
        root.go(screen, window, cx);
        root
    }

    /// A batch from the hub: re-read what is shown if the batch touches it; the tabs when the list or a stack moved.
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        self.refresh_tabs(cx);
        self.report_tasks(cx);
        self.rebuild_attention(cx);
        match &self.screen {
            Screen::Library => {
                if !batch.hub.is_empty() || !batch.stacks.is_empty() {
                    self.library.update(cx, |l, cx| l.refresh(cx));
                }
            }
            Screen::Empty(_) => {}
            Screen::Activity(_) | Screen::Settings => self.apply_pages(batch, cx),
            Screen::Stack(k) => {
                if !batch.parts(k).is_empty() || !batch.hub.is_empty() {
                    self.overview.update(cx, |o, cx| o.refresh(cx));
                }
            }
            Screen::Instance(k, _) | Screen::Logs(k) => {
                if !batch.parts(k).is_empty() {
                    self.apply_inside(batch, cx);
                    cx.notify();
                }
            }
        }
        self.wind(cx);
    }

    /// Reads what is shown again; redraws only what changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_tabs(cx);
        match &self.screen {
            Screen::Library => self.library.update(cx, |l, cx| l.refresh(cx)),
            Screen::Empty(_) => cx.notify(),
            Screen::Activity(_) | Screen::Settings => cx.notify(),
            Screen::Stack(_) => self.overview.update(cx, |o, cx| o.refresh(cx)),
            Screen::Instance(..) | Screen::Logs(_) => cx.notify(),
        }
    }

    /// The stack shown, if a stack's page (or a page inside it) is.
    pub(crate) fn key(&self) -> Option<&StackKey> {
        self.screen.key()
    }

    /// Shows the stack named `name`, and selects the card `show` on its Map.
    pub(crate) fn show(
        &mut self,
        name: &str,
        show: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(v) = self.hub.stacks().into_iter().find(|s| s.name == name) else {
            return false;
        };
        self.go(Screen::Stack(v.key), window, cx);
        if let Some(card) = show {
            self.select(card, cx);
        }
        true
    }

    /// Shows the stack `key`.
    pub(crate) fn show_key(&mut self, key: StackKey, window: &mut Window, cx: &mut Context<Self>) {
        self.go(Screen::Stack(key), window, cx);
    }

    /// Selects the card `name` on the Map, centred.
    pub(crate) fn select(&mut self, name: &str, cx: &mut Context<Self>) {
        let found = self.map.update(cx, |m, cx| {
            let id = m.card_named(name);
            if let Some(id) = id {
                m.jump_to(id, cx);
            }
            id.is_some()
        });
        self.overview.update(cx, |o, cx| o.select(name, cx));
        if !found {
            self.say(format!("{name} is not on this stack's map."), cx);
        }
    }

    /// The stack doze runs from `dir`, and the card `show` on it.
    pub(crate) fn open_dir(
        &mut self,
        dir: &std::path::Path,
        show: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = if dir.is_dir() {
            dir.to_path_buf()
        } else {
            dir.parent().map(PathBuf::from).unwrap_or_default()
        };
        match self
            .hub
            .stacks()
            .into_iter()
            .find(|s| std::path::Path::new(&s.dir) == dir)
        {
            Some(s) => {
                self.go(Screen::Stack(s.key), window, cx);
                if let Some(name) = show {
                    self.select(name, cx);
                }
            }
            None => self.open_new(&dir, window, cx),
        }
    }

    /// Says something: a toast, kept among the recent lines.
    pub(crate) fn say(&mut self, words: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = words.into().to_string();
        self.acts.update(cx, |a, cx| a.say(text, false, cx));
    }

    /// A folder (or a file in it, a doze.hcl) dropped on the window: its stack, if doze runs one there.
    fn dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = paths.paths().first() {
            self.open_dir(&p.clone(), None, window, cx);
        }
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "fixtures")]
        crate::driver::frame();
        crate::motion::set_reduced(cx.reduce_motion());
        crate::motion::frame();
        self.follow_system(window, cx);
        let t = tokens(cx);
        let page = match &self.screen {
            Screen::Library => self.library.clone().into_any_element(),
            Screen::Empty(_) => self
                .empty
                .clone()
                .map_or_else(|| div().into_any_element(), IntoElement::into_any_element),
            Screen::Stack(_) => self.overview.clone().into_any_element(),
            Screen::Instance(..) | Screen::Logs(_) => self
                .inside_el(&t, cx)
                .unwrap_or_else(|| div().into_any_element()),
            Screen::Activity(_) | Screen::Settings => {
                self.pages_el().unwrap_or_else(|| div().into_any_element())
            }
        };
        let page = div()
            .key_context("Page")
            .flex()
            .flex_col()
            .size_full()
            .child(titlebar::titlebar(&t, self, cx))
            .children(self.away_el(cx))
            .child(div().flex().flex_1().min_h_0().child(page));
        div()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context("Root")
            .bg(t.bg)
            .text_color(t.text)
            .text_size(px(ty::BASE))
            .line_height(px(ty::LH))
            .font_family(UI_FAMILY)
            .on_action(cx.listener(Self::on_palette))
            .on_action(cx.listener(Self::on_help))
            .on_action(cx.listener(Self::on_bell))
            .map(|d| self.on_history(d, cx))
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| this.close(window, cx)))
            .on_action(
                cx.listener(|this, _: &GoHome, window, cx| this.go(Screen::Library, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NextPalette, window, cx| this.next_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CycleMode, window, cx| this.cycle_mode(window, cx)))
            .on_action(cx.listener(|this, _: &CycleDensity, _, cx| this.cycle_density(cx)))
            .on_action(cx.listener(|this, _: &AllLogs, window, cx| this.open_logs(window, cx)))
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .map(|d| self.on_acting(d, cx))
            .map(|d| self.on_pages(d, cx))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.palette_step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.palette_step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.palette_choose(window, cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.dropped(paths, window, cx)
            }))
            .child(page)
            .child(self.foot_column(cx))
            .children(self.overlay_el(cx))
    }
}
