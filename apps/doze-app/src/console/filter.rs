//! A console table's ordering and filter: a click on a column's title orders the table by it; `/` opens a filter
//! box under the focused table, Enter gives the keys back to the table, Esc lets the filter go.

use gpui::{Context, Window, prelude::*};
use gpui_component::input::{InputEvent, InputState};

use super::ConsoleTabView;

impl ConsoleTabView {
    /// A click on a column's title: the table ordered by it, upwards, then downwards, then as the module sent it.
    pub(crate) fn sort(&mut self, id: &str, col: usize, cx: &mut Context<Self>) {
        self.focused = Some(id.to_owned());
        self.views.entry(id.to_owned()).or_default().click(col);
        self.refresh(cx);
        cx.notify();
    }

    /// `/`: the filter box of the focused table (or of the first long one), with the keys in it.
    pub(super) fn open_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.focused_table() else {
            return;
        };
        let input = match &self.filter {
            Some((t, i)) if *t == id => i.clone(),
            _ => {
                let i = cx.new(|cx| InputState::new(window, cx).placeholder("Filter the rows"));
                let words = self
                    .views
                    .get(&id)
                    .map(|v| v.filter.clone())
                    .unwrap_or_default();
                if !words.is_empty() {
                    i.update(cx, |i, cx| i.set_value(words, window, cx));
                }
                cx.subscribe_in(
                    &i,
                    window,
                    |v: &mut ConsoleTabView, i, ev: &InputEvent, window, cx| match ev {
                        InputEvent::Change => {
                            let words = i.read(cx).value().to_string();
                            if let Some((id, _)) = v.filter.clone() {
                                v.set_filter(&id, &words, cx);
                            }
                        }
                        InputEvent::PressEnter { .. } => window.focus(&v.focus, cx),
                        _ => {}
                    },
                )
                .detach();
                self.filter = Some((id, i.clone()));
                i
            }
        };
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    pub(super) fn set_filter(&mut self, id: &str, words: &str, cx: &mut Context<Self>) {
        let view = self.views.entry(id.to_owned()).or_default();
        if view.filter == words {
            return;
        }
        words.clone_into(&mut view.filter);
        self.refresh(cx);
        // The selection stays on a row the filter shows.
        if let Some((_, rows)) = self.tables().into_iter().find(|(t, _)| t == id)
            && let Some(&first) = rows.first()
            && !rows.contains(&self.sel.get(id).copied().unwrap_or(0))
        {
            self.pick(id, first, cx);
        }
        cx.notify();
    }

    /// The filter box shown with `words` in it (a shot).
    #[cfg(feature = "capture")]
    pub(crate) fn filter_words(
        &mut self,
        words: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_filter(window, cx);
        if let Some((id, i)) = self.filter.clone() {
            i.update(cx, |i, cx| i.set_value(words.to_owned(), window, cx));
            self.set_filter(&id, words, cx);
        }
    }

    /// Esc in the filter box: the filter is let go and the keys go back to the table.
    pub(super) fn close_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, _)) = self.filter.take() {
            self.set_filter(&id, "", cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
