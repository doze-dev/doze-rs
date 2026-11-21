//! Where the Map's keys are: its text boxes (whose frames ease to the accent's tint) and its cards, each a Tab stop as
//! Go's card buttons are. A card reached by Tab is not selected yet: ⏎ selects it, ⏎ again opens it, and space wakes
//! or sleeps it (Go flow.tsx `card` onKeyDown); a click selects as before.

use gpui::{Context, Entity, Window};
use gpui_component::input::InputState;

use super::MapView;

impl MapView {
    /// Notes where the keys are (each render): the text boxes, and the card that has them. Cards get their Tab stops
    /// once per layout; only the cards drawn are stops (GPUI tabs through what it painted).
    pub(crate) fn note_keys(&mut self, window: &Window, cx: &mut Context<Self>) {
        let on = |i: Option<&Entity<InputState>>| {
            i.is_some_and(|i| crate::widgets::has_keys(i, window, cx))
        };
        self.keys_in = (
            on(self.search.as_ref()),
            on(self.list.as_ref().map(|l| &l.input)),
        );
        let names = &mut self.card_keys.0;
        if names.len() != self.layout.cards.len()
            || self
                .layout
                .cards
                .iter()
                .any(|c| !names.contains_key(&c.name))
        {
            // Tab goes through them in the layout's order (the drawing's reading order), not the order they are painted in.
            *names = (self.layout.cards.iter().enumerate())
                .map(|(i, c)| {
                    let h = cx.focus_handle().tab_stop(true).tab_index(i as isize);
                    (c.name.clone(), h)
                })
                .collect();
        }
        let focused = names
            .iter()
            .find(|(_, h)| h.is_focused(window))
            .map(|(n, _)| n.clone());
        self.card_keys.1 = focused.and_then(|n| self.card_named(&n));
    }

    /// The card's name with the keys on it but not yet selected: space acts on it (Go: space on a focused card).
    pub(crate) fn keyed_card(&self) -> Option<String> {
        let id = self.card_keys.1.filter(|id| Some(*id) != self.lit)?;
        self.layout.card(id).map(|c| c.name.clone())
    }

    /// ⏎: a find hit flies to its card; a card with the keys and not selected is selected (Go: "⏎ selects it");
    /// the selected card opens; with none, the first is selected.
    pub(crate) fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(&id) = self.hits.get(self.hit) {
            self.jump_to(id, cx);
        } else if let Some(id) = self.card_keys.1.filter(|id| Some(*id) != self.lit) {
            self.select(Some(id), cx);
        } else if let Some(c) = self.lit.and_then(|id| self.layout.card(id)) {
            if self.running {
                cx.emit(super::MapEvent::Open(c.name.clone()));
            }
        } else if self.lit.is_none() {
            self.step(1.0, 0.0, cx);
        }
    }
}
