//! The page's selection: a click or a key on a row or tile, and the Map's card (Go `select`, `pick`).

use gpui::Context;

use super::{OverviewView, View};

impl OverviewView {
    pub(crate) fn select(&mut self, name: &str, cx: &mut Context<Self>) {
        self.selected = Some(name.to_owned());
        if let Some(st) = &self.stack
            && st.running
        {
            self.pane = doze_desk::overview::pane(st, name, self.hub.now());
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
