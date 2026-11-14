//! The page's selection: a click or a key on a row or tile, and the Map's card (Go `select`, `pick`).

use gpui::{App, Context, KeyDownEvent, Window};

use super::{OverviewView, View};

impl OverviewView {
    pub(crate) fn select(&mut self, name: &str, cx: &mut Context<Self>) {
        self.selected = Some(name.to_owned());
        if let Some(st) = &self.stack
            && st.running
        {
            self.pane = doze_desk::overview::pane(st, name, self.hub.now());
            if self.logs.is_none() {
                self.logs = Some(self.hub.watch(&st.key, doze_control::Topic::Logs));
            }
        }
        cx.notify();
    }

    pub(super) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
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

    /// The Map's selection is the page's on the Map (Go: its keys act on `lit`): a card let go lets the row go.
    pub(super) fn map_selected(&mut self, name: Option<&str>, cx: &mut Context<Self>) {
        match name {
            Some(n) => self.select(n, cx),
            None if self.view == View::Map => {
                self.selected = None;
                cx.notify();
            }
            None => {}
        }
    }
}

impl OverviewView {
    /// Go's `Name` button on a tile or a row: each shown name is a Tab stop; the one that takes the keys selects its
    /// service (Go `onFocus`), and ⏎ then opens it (the page's own ⏎ opens the selection). Handles are kept by name
    /// for the services shown, made once each.
    pub(super) fn keep_names(&mut self, window: &Window, cx: &mut Context<Self>) {
        let shown: Vec<String> = self.shown().iter().map(|r| r.name.clone()).collect();
        let names = &mut self.names.0;
        names.retain(|n, _| shown.contains(n));
        for n in &shown {
            names
                .entry(n.clone())
                .or_insert_with(|| cx.focus_handle().tab_stop(true));
        }
        let keys = names
            .iter()
            .find(|(_, h)| h.is_focused(window))
            .map(|(n, _)| n.clone());
        let moved = keys.is_some() && keys != self.names.1;
        self.names.1 = keys.clone();
        if let (true, Some(n)) = (moved, keys)
            && self.selected.as_deref() != Some(n.as_str())
        {
            self.select(&n, cx);
        }
    }

    /// The name `name`'s handle, and whether it has the keys (its ring shows).
    pub(super) fn name_keys(&self, name: &str) -> (Option<&gpui::FocusHandle>, bool) {
        (
            self.names.0.get(name),
            self.names.1.as_deref() == Some(name),
        )
    }
}

/// Tab and ⇧Tab move the keys to the next or the last name (a browser's Tab over Go's name buttons).
pub(super) fn tab(ev: &KeyDownEvent, window: &mut Window, cx: &mut App) {
    let k = &ev.keystroke;
    if k.key != "tab" || k.modifiers.control || k.modifiers.platform || k.modifiers.alt {
        return;
    }
    if k.modifiers.shift {
        window.focus_prev(cx);
    } else {
        window.focus_next(cx);
    }
    cx.stop_propagation();
}

/// Go `.nm:focus-visible`: a ring 2 px out from the name, the accent at 55%, while it has the keys.
pub(super) fn name_ring(
    t: &crate::theme::Tokens,
    name: gpui::Div,
    (handle, keys): (Option<&gpui::FocusHandle>, bool),
) -> gpui::Div {
    use gpui::{Styled as _, prelude::*, px};
    let mut d = gpui::div().relative().min_w_0().flex().child(name);
    if let Some(h) = handle {
        d = d.track_focus(h);
    }
    d.when(keys, |d| {
        d.child(
            gpui::div()
                .absolute()
                .top(px(-3.))
                .bottom(px(-3.))
                .left(px(-3.))
                .right(px(-3.))
                .rounded(px(4.))
                .border_1()
                .border_color(crate::theme::alpha(t.accent, 0.55)),
        )
    })
}
