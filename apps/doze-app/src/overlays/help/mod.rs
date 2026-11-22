//! The help sheet (`?`, ⌘/): the keys of the page it was opened on, with Everywhere's beside them, a rail to every
//! other context and to the words, and a filter across all of them. Every key comes from the action table
//! ([`crate::actions::help`]), so the sheet lists exactly what is bound and cannot drift from the keymap. Esc clears
//! the filter first and closes the sheet second; ↑ ↓ move through the rail.

mod chrome;
mod draw;
mod groups;
pub(crate) mod model;
#[cfg(test)]
mod tests;

use gpui::{Context, Entity, IntoElement, Render, ScrollHandle, Window, div, prelude::*, px};
use gpui_component::input::{InputEvent, InputState};

use crate::actions::{MoveDown, MoveUp, Scope};
use crate::theme::{tokens, ty};
use crate::widgets::UI_FAMILY;
use model::{Found, Section};

/// The sheet open over the page.
pub(crate) struct HelpView {
    pub(crate) input: Entity<InputState>,
    /// The context of the page the sheet was opened on.
    pub(crate) here: Scope,
    /// The section shown while nothing is typed.
    pub(crate) on: Section,
    /// What the filter found (empty while nothing is typed).
    pub(crate) found: Found,
    scroll: ScrollHandle,
}

impl HelpView {
    /// The sheet, opened on `here`'s keys, the keys in its filter.
    pub(crate) fn new(here: Scope, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter keys and words"));
        cx.subscribe(&input, |this: &mut HelpView, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.refilter(cx);
            }
        })
        .detach();
        input.update(cx, |i, cx| i.focus(window, cx));
        HelpView {
            input,
            here,
            on: model::opening(here),
            found: Found::default(),
            scroll: ScrollHandle::new(),
        }
    }

    /// What is in the filter.
    pub(crate) fn query(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Whether something is typed in the filter.
    pub(crate) fn filtering(&self, cx: &gpui::App) -> bool {
        !self.query(cx).trim().is_empty()
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.found = model::find(&self.query(cx));
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    /// Esc: clears the filter, else (false) the sheet closes.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.query(cx).is_empty() {
            return false;
        }
        self.input.update(cx, |i, cx| i.set_value("", window, cx));
        self.refilter(cx);
        true
    }

    /// ↑ ↓: the section before or after on the rail (while nothing is typed: the filter's matches are all in view).
    pub(crate) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.filtering(cx) {
            return;
        }
        let next = self.on.step(by);
        if next != self.on {
            self.on = next;
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
            cx.notify();
        }
    }

    /// A rail entry clicked: its section, the filter cleared.
    pub(crate) fn show(&mut self, s: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.on = s;
        if !self.query(cx).is_empty() {
            self.input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.refilter(cx);
    }

    /// Puts `q` in the filter (a shot's, a test's).
    #[cfg(feature = "capture")]
    pub(crate) fn type_in(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |i, cx| i.set_value(q.to_owned(), window, cx));
        self.refilter(cx);
    }
}

/// The panel's widest, its tallest, the margins around it, and the room under the title bar.
const WIDE: f32 = 1120.;
const TALL: f32 = 760.;
const INSET: f32 = 20.;
const INSET_TOP: f32 = 42. + 22.;
/// The rail's width, and the gap between it and the keys.
const RAIL_W: f32 = 176.;
const PAD: f32 = 24.;
/// The keys' area must be this wide to set two columns side by side.
const TWO_UP: f32 = 680.;

impl Render for HelpView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let width = (vw - 2. * INSET).min(WIDE);
        let height = (vh - INSET_TOP - INSET).min(TALL);
        let two = width - 2. * PAD - RAIL_W - PAD >= TWO_UP;
        let q = self.query(cx);
        let filtering = !q.trim().is_empty();
        let body = if filtering {
            draw::found(&t, &self.found, q.trim(), two)
        } else {
            draw::section(&t, self.on, two)
        };
        let panel = div()
            .key_context("Help")
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.step(-1, cx)))
            .w(px(width))
            .h(px(height))
            .flex()
            .flex_col()
            .font_family(UI_FAMILY)
            .text_size(px(ty::BASE))
            .line_height(px(ty::LH))
            .overflow_hidden()
            .rounded(px(14.))
            .relative()
            .child(super::scrim::grown_frame(&t, t.line2, 14., "help-frame"))
            .child(chrome::head(&t, &self.input, window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(chrome::rail(&t, self, filtering, cx))
                    .child(
                        div()
                            .id("help-body")
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .px(px(PAD))
                            .pt(px(14.))
                            .pb(px(12.))
                            .child(body),
                    ),
            )
            .child(chrome::foot(&t, crate::VERSION, filtering));
        // Inset below the title bar (its tabs stay in sight) and off the window's edges.
        div()
            .absolute()
            .top(px(INSET_TOP))
            .bottom(px(INSET))
            .left(px(INSET))
            .right(px(INSET))
            .flex()
            .justify_center()
            .items_start()
            .child(panel)
    }
}
