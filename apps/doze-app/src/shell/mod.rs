//! The window's root: the title bar (Home and a tab per stack), the page shown (the Library, or a stack's page in
//! one of its four views), a line at the foot for a moment, and the overlays over it all. It reads the hub: a batch
//! that touches what is shown re-reads it, and a page re-renders only when what it shows changed (the views are
//! `PartialEq`), which is what keeps an idle window at zero redraws.

mod nav;
mod ops;
mod titlebar;

use std::path::PathBuf;
use std::time::Duration;

use doze_desk::{Batch, Hub, StackKey};
use gpui::{
    AppContext as _, Context, Entity, ExternalPaths, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Window, div, prelude::*, px,
};

use crate::actions::{
    CloseOverlay, CloseWindow, Confirm, CycleDensity, CycleMode, GoHome, MoveDown, MoveUp,
    NextPalette, Quit,
};
use crate::facts::{self, Facts};
use crate::library::{LibraryEvent, LibraryView};
use crate::map::MapView;
use crate::overlays::Overlay;
use crate::overview::{OverviewEvent, OverviewView};
use crate::theme::tokens;
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
    /// A line at the foot, for a moment (a verb that comes later, a link doze cannot follow yet).
    notice: Option<SharedString>,
    focus: FocusHandle,
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
        let facts = cx.new(|_| Facts::new(mode));
        let library = cx.new(|cx| LibraryView::new(hub.clone(), facts.clone(), cx));
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
        cx.subscribe(&overview, |this, _, ev: &OverviewEvent, cx| match ev {
            OverviewEvent::Say(s) => this.say(s.clone(), cx),
        })
        .detach();
        let map = overview.read(cx).map.clone();
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
            notice: None,
            focus: cx.focus_handle(),
        };
        let screen = first.map_or(Screen::Library, Screen::Stack);
        root.go(screen, window, cx);
        root
    }

    /// A batch from the hub: re-read what is shown if the batch touches it; the tabs when the list or a stack moved.
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        self.refresh_tabs(cx);
        match &self.screen {
            Screen::Library => {
                if !batch.hub.is_empty() || !batch.stacks.is_empty() {
                    self.library.update(cx, |l, cx| l.refresh(cx));
                }
            }
            Screen::Stack(k) => {
                if !batch.parts(k).is_empty() || !batch.hub.is_empty() {
                    self.overview.update(cx, |o, cx| o.refresh(cx));
                }
            }
        }
    }

    /// Reads what is shown again; redraws only what changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_tabs(cx);
        match &self.screen {
            Screen::Library => self.library.update(cx, |l, cx| l.refresh(cx)),
            Screen::Stack(_) => self.overview.update(cx, |o, cx| o.refresh(cx)),
        }
    }

    /// The stack shown, if a stack's page is.
    pub(crate) fn key(&self) -> Option<&StackKey> {
        match &self.screen {
            Screen::Stack(k) => Some(k),
            Screen::Library => None,
        }
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
            None => self.say(
                format!(
                    "doze has no stack in {} yet. Adding a folder comes with the next step (M2).",
                    dir.display()
                ),
                cx,
            ),
        }
    }

    /// Says something at the foot for a few seconds.
    pub(crate) fn say(&mut self, words: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(words.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |r, cx| {
                r.notice = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// A folder (or a file in it, a doze.hcl) dropped on the window: its stack, if doze runs one there.
    fn dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = paths.paths().first() {
            self.open_dir(&p.clone(), None, window, cx);
        }
    }

    fn notice_el(&self, t: &crate::theme::Tokens) -> Option<gpui::AnyElement> {
        let n = self.notice.clone()?;
        Some(
            div()
                .absolute()
                .bottom(px(46.))
                .left(px(0.))
                .right(px(0.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .max_w(px(720.))
                        .px(px(14.))
                        .py(px(8.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(t.line2)
                        .bg(t.panel)
                        .shadow(vec![crate::overlays::shadow(t)])
                        .text_color(t.text)
                        .child(n),
                )
                .into_any_element(),
        )
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_system(window, cx);
        let t = tokens(cx);
        let page = match &self.screen {
            Screen::Library => self.library.clone().into_any_element(),
            Screen::Stack(_) => self.overview.clone().into_any_element(),
        };
        let page = div()
            .key_context("Page")
            .flex()
            .flex_col()
            .size_full()
            .child(titlebar::titlebar(&t, self, cx))
            .child(div().flex_1().min_h_0().child(page));
        div()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context("Root")
            .bg(t.bg)
            .text_color(t.text)
            .text_size(px(13.))
            .line_height(px(20.))
            .font_family(UI_FAMILY)
            .on_action(cx.listener(Self::on_palette))
            .on_action(cx.listener(Self::on_help))
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| this.close(window, cx)))
            .on_action(
                cx.listener(|this, _: &GoHome, window, cx| this.go(Screen::Library, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NextPalette, window, cx| this.next_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CycleMode, window, cx| this.cycle_mode(window, cx)))
            .on_action(cx.listener(|this, _: &CycleDensity, _, cx| this.cycle_density(cx)))
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.palette_step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.palette_step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.palette_choose(cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.dropped(paths, window, cx)
            }))
            .child(page)
            .children(self.notice_el(&t))
            .children(self.overlay_el(cx))
    }
}
