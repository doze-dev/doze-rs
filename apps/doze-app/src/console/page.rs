//! The tab's frame: what it holds (the tree of blocks, or why there is none yet) and the quiet line an instance that
//! sleeps gets.

use gpui::{Context, IntoElement, Window, div, prelude::*, px};

use super::{ConsoleTabView, blocks};
use crate::instance::ui::text;
use crate::theme::ty;

impl ConsoleTabView {
    /// What the tab holds: the tree, or why there is none yet.
    pub(super) fn body(
        &self,
        t: &crate::theme::Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = &self.page;
        let why = if p.reading && !p.err.is_empty() {
            // The banner above says why; not "reading", which it is not.
            Some("Nothing to show until the module answers.")
        } else if p.reading {
            Some("Reading the console…")
        } else if p.missing {
            Some("The module has no such tab.")
        } else {
            None
        };
        match why {
            // Said where the blocks would start, not floating mid-page.
            Some(w) => div()
                .flex_1()
                .px(px(28.))
                .pt(px(16.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(w)
                .into_any_element(),
            None => blocks::tab_body(t, self, window, cx),
        }
    }

    /// Asleep, the console shows what it last read and never wakes the module: one quiet line says so.
    pub(super) fn asleep(&self, t: &crate::theme::Tokens) -> Option<gpui::Div> {
        let p = &self.page;
        (!p.state.is_empty() && !p.awake && !p.missing && !p.reading).then(|| {
            div()
                .flex_none()
                .px(px(28.))
                .pt(px(12.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(text(&format!(
                    "{} is {}: the console never wakes it, so this is what it showed last.",
                    p.name,
                    doze_display::State::parse(&p.state).word()
                )))
        })
    }
}

/// Words as a sentence starts: a capital first ("the module did not answer" is "The module did not answer").
pub(super) fn sentence(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
