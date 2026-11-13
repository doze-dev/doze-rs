//! Where the window is: the Library, or a stack's page; and the title bar's tabs (every running stack, and the
//! stopped ones opened this session, as the Go app kept them).

use doze_desk::StackKey;
use doze_display as words;
use gpui::{Context, Focusable, Window};

use super::Root;
use crate::overview::View;

/// The page shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Screen {
    /// The first tab: every stack.
    Library,
    /// A stack's page.
    Stack(StackKey),
    /// An instance's page, beside the stack's sidebar.
    Instance(StackKey, String),
    /// All logs of a stack, merged, beside its sidebar.
    Logs(StackKey),
    /// A folder with no doze.hcl yet: the three ways to start.
    Empty(std::path::PathBuf),
}

impl Screen {
    /// The stack the screen is in, if any.
    pub(crate) fn key(&self) -> Option<&StackKey> {
        match self {
            Screen::Library | Screen::Empty(_) => None,
            Screen::Stack(k) | Screen::Instance(k, _) | Screen::Logs(k) => Some(k),
        }
    }
}

/// A stack's tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tab {
    pub(crate) key: StackKey,
    pub(crate) name: String,
    pub(crate) running: bool,
    /// Something in it needs you.
    pub(crate) trouble: bool,
}

/// The sidebar's foot, as Go's: "doze v0.1.2 · 2 stacks".
pub(super) fn foot(hub: &doze_desk::Hub) -> String {
    let stacks = hub.stacks().len();
    format!(
        "doze {} · {stacks} {}",
        crate::VERSION,
        if stacks == 1 { "stack" } else { "stacks" }
    )
}

impl Root {
    /// Shows `screen`: its page reads the hub, the other stops watching, the keys go to it.
    pub(crate) fn go(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(k) = screen.key() {
            if !self.opened.contains(k) {
                self.opened.push(k.clone());
            }
            self.hidden.retain(|h| h != k);
        }
        match &screen {
            Screen::Instance(k, name) => {
                if !self.enter_instance(k, name, window, cx) {
                    return;
                }
                self.overview
                    .update(cx, |o, cx| o.show(Some(k.clone()), cx));
                self.overview.update(cx, |o, _| o.hide());
            }
            Screen::Logs(k) => {
                self.overview
                    .update(cx, |o, cx| o.show(Some(k.clone()), cx));
                self.overview.update(cx, |o, _| o.hide());
                self.enter_logs(k, window, cx);
            }
            Screen::Stack(k) => {
                self.inside = None;
                let key = k.clone();
                self.overview.update(cx, |o, cx| {
                    o.show(Some(key), cx);
                    o.refresh(cx);
                });
                let o = self.overview.read(cx);
                let f = if o.view == View::Map {
                    o.map.read(cx).focus.clone()
                } else {
                    o.focus.clone()
                };
                window.focus(&f, cx);
            }
            Screen::Empty(_) => {
                self.inside = None;
                self.overview.update(cx, |o, _| o.hide());
                if let Some(e) = &self.empty {
                    let f = e.read(cx).focus.clone();
                    window.focus(&f, cx);
                }
            }
            Screen::Library => {
                self.inside = None;
                self.overview.update(cx, |o, _| o.hide());
                self.library.update(cx, |l, cx| l.refresh(cx));
                let f = self.library.focus_handle(cx);
                window.focus(&f, cx);
            }
        }
        self.moved(&screen);
        self.screen = screen;
        self.refresh_tabs(cx);
        cx.notify();
    }

    /// Closes a stopped stack's tab (a running stack's tab stays while it runs).
    pub(crate) fn close_tab(
        &mut self,
        key: &StackKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.opened.retain(|k| k != key);
        // A running stack's tab stays put away until the stack is opened again (it keeps running).
        if !self.hidden.contains(key) {
            self.hidden.push(key.clone());
        }
        if self.key() == Some(key) {
            self.go(Screen::Library, window, cx);
        } else {
            self.refresh_tabs(cx);
        }
    }

    /// The tabs, from the hub; redraws only when they changed.
    pub(crate) fn refresh_tabs(&mut self, cx: &mut Context<Self>) {
        let tabs: Vec<Tab> = self
            .hub
            .stacks()
            .into_iter()
            .filter(|s| {
                (s.running && !self.hidden.contains(&s.key)) || self.opened.contains(&s.key)
            })
            .map(|s| {
                let snap = s.words();
                Tab {
                    trouble: snap.instances.iter().any(words::troubled),
                    key: s.key,
                    name: s.name,
                    running: s.running,
                }
            })
            .collect();
        if tabs != self.tabs {
            self.tabs = tabs;
            cx.notify();
        }
    }
}
