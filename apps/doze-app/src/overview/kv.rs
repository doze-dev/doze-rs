//! `.kv`: a row of a card in the List's pane (a key, a value, and Copy or a verb), and `.copy`, the small accent word
//! that acts.

use gpui::{AnyElement, ClipboardItem, Context, IntoElement, div, prelude::*, px};

use super::{M2, OverviewView};
use crate::kit::s;
use crate::theme::Tokens;
use crate::widgets::{MONO_FAMILY, w};

/// `.kv`: the key, the value, and Copy when there is something to copy.
pub(super) struct Kv<'a> {
    pub(super) k: &'a str,
    pub(super) v: &'a str,
    pub(super) copy: Option<String>,
    /// The value is the thing itself (`.v.b`), not a word about it.
    pub(super) bright: bool,
    /// A sentence, wrapped, in the page's face; else one mono line.
    pub(super) wrap: bool,
    /// A verb beside the value in place of Copy (`Pin`), which comes with M2.
    pub(super) verb: Option<&'static str>,
}

impl<'a> Kv<'a> {
    pub(super) fn new(k: &'a str, v: &'a str) -> Kv<'a> {
        Kv {
            k,
            v,
            copy: None,
            bright: false,
            wrap: false,
            verb: None,
        }
    }

    pub(super) fn copy(mut self, c: &str) -> Kv<'a> {
        self.copy = Some(c.to_owned());
        self.bright = true;
        self
    }

    pub(super) fn el(self, t: &Tokens, cx: &mut Context<OverviewView>) -> AnyElement {
        let value = div()
            .flex_1()
            .min_w_0()
            .text_size(px(12.))
            .text_color(if self.bright { t.bright } else { t.text })
            .child(s(self.v));
        let value = if self.wrap {
            value
        } else {
            value
                .font_family(MONO_FAMILY)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
        };
        let row = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .min_h(px(22.))
            .text_size(px(12.))
            .child(
                div()
                    .w(px(84.))
                    .flex_none()
                    .text_color(t.dim)
                    .child(s(self.k)),
            )
            .child(value);
        if let Some(verb) = self.verb {
            let id = gpui::SharedString::from(format!("verb-{}", self.k));
            return row
                .child(link(t, id, verb).on_click(cx.listener(|this, _, _, cx| this.say(M2, cx))))
                .into_any_element();
        }
        let Some(c) = self.copy else {
            return row.into_any_element();
        };
        let what = self.k.to_lowercase();
        row.child(
            div()
                .id(gpui::SharedString::from(format!("copy-{}", self.k)))
                .flex_none()
                .text_size(px(11.))
                .font_weight(w(500.))
                .text_color(t.accent)
                .cursor_pointer()
                .hover(|d| d.underline())
                .child("Copy")
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(c.clone()));
                    this.say(&format!("Copied the {what}"), cx);
                })),
        )
        .into_any_element()
    }
}

/// `.copy`: a small accent word that acts (Copy, Pin).
fn link(t: &Tokens, id: gpui::SharedString, word: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .text_size(px(11.))
        .font_weight(w(500.))
        .text_color(t.accent)
        .cursor_pointer()
        .hover(|d| d.underline())
        .child(word)
}
