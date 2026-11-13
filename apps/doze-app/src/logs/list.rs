//! The lines, virtualised: a uniform list of 24px rows, of which only those in view are built, each read from the
//! ring as it is drawn. Drawn as the Logs board draws them: the time, the instance in its own colour on the merged
//! page, the level tag, the text with the filter's matches marked. Scrolling up pauses; back at the end it follows.

use std::ops::Range;

use doze_desk::page::logs::Line;
use gpui::{
    AnyElement, Context, HighlightStyle, IntoElement, MouseButton, MouseDownEvent, ScrollEvent,
    SharedString, StyledText, div, prelude::*, px, uniform_list,
};

use super::LogsView;
use crate::instance::ui::{self, text};
use crate::theme::Tokens;
use crate::widgets::{MONO_FAMILY, w};

/// A row's height.
pub(crate) const ROW: f32 = 24.;

pub(crate) fn list(t: &Tokens, v: &mut LogsView, cx: &mut Context<LogsView>) -> AnyElement {
    let n = v.index.len();
    if n == 0 {
        let said = if v.filter_text.is_empty() && v.level <= 1 {
            "nothing yet · lines appear here as they are printed"
        } else {
            "no line matches · fewer filters show more"
        };
        return div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .text_color(t.dim)
            .child(said)
            .into_any_element();
    }
    let t = *t;
    let rows = uniform_list(
        "log-lines",
        n,
        cx.processor(move |v: &mut LogsView, range: Range<usize>, _, cx| v.rows(&t, range, cx)),
    )
    .track_scroll(&v.scroll)
    .flex_1()
    .py(px(4.));
    div()
        .id("log-dock")
        .overflow_hidden()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .font_family(MONO_FAMILY)
        .text_size(px(12.))
        .on_scroll(cx.listener(|v, e: &ScrollEvent, window, cx| v.wheel(e, window, cx)))
        .child(rows)
        .into_any_element()
}

impl LogsView {
    /// The rows `range`, read from the ring now.
    fn rows(&mut self, t: &Tokens, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.drawn = range.clone();
        let (idx, now, zone) = (&self.index, self.now, self.zone);
        let lines = self
            .hub
            .with_logs(&self.key, |r| idx.rows(r, range.clone(), now, zone))
            .unwrap_or_default();
        let picked = self.picked.map(|(a, b)| (a.min(b), a.max(b)));
        lines
            .into_iter()
            .zip(range)
            .map(|(l, row)| {
                let anchor = self.index.anchor(row);
                let on = picked.is_some_and(|(a, b)| anchor >= a && anchor <= b);
                let current = self.hit.is_some_and(|(h, _)| h == row);
                self.row(t, &l, row, on || current, cx)
            })
            .collect()
    }

    /// One line's row: the time, the instance on the merged page, the level tag, the text.
    fn row(
        &self,
        t: &Tokens,
        l: &Line,
        row: usize,
        on: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let merged = self.instance.is_empty();
        let when = if self.clock { &l.time } else { &l.rel };
        let who = self.who(t, l);
        let body = self.body(t, l);
        div()
            .id(("line", row))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(ROW))
            .px(px(12.))
            .whitespace_nowrap()
            .overflow_hidden()
            .when(on, |d| d.bg(t.pick))
            .hover(|s| s.bg(t.sel))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |v, e: &MouseDownEvent, _, cx| v.pick(row, e.modifiers.shift, cx)),
            )
            .child(
                div()
                    .w(px(62.))
                    .flex_none()
                    .text_color(t.dim)
                    .child(text(when)),
            )
            .when(merged, |d| {
                d.child(
                    div()
                        .w(px(104.))
                        .flex_none()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_weight(w(500.))
                        .text_color(who)
                        .child(text(&l.instance)),
                )
            })
            .child(
                div()
                    .w(px(34.))
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(w(600.))
                    .text_color(ui::ink(t, l.level_ink()))
                    .child(text(l.level)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(ui::ink(t, l.ink()))
                    .child(body),
            )
            .into_any_element()
    }

    /// The colour of the instance a line came from: doze's own accent, or its hue.
    fn who(&self, t: &Tokens, l: &Line) -> gpui::Hsla {
        let hue = [t.cool, t.accent, t.good, t.warn, t.extra, t.text];
        if l.doze {
            t.accent
        } else {
            hue.get(self.index.hue(&l.instance))
                .copied()
                .unwrap_or(t.text)
        }
    }

    /// A line's text, with the filter's matches marked.
    fn body(&self, t: &Tokens, l: &Line) -> AnyElement {
        let spans = self.index.query().filter.spans(&l.text);
        if spans.is_empty() {
            text(&l.text).into_any_element()
        } else {
            let hl = HighlightStyle {
                background_color: Some(t.warn),
                color: Some(t.ink),
                ..HighlightStyle::default()
            };
            StyledText::new(SharedString::from(l.text.clone()))
                .with_highlights(spans.into_iter().map(|r| (r, hl)))
                .into_any_element()
        }
    }

    /// A click picks a line; a shift-click picks the lines from the last one picked.
    fn pick(&mut self, row: usize, extend: bool, cx: &mut Context<Self>) {
        let a = self.index.anchor(row);
        self.picked = match (self.picked, extend) {
            (Some((first, _)), true) => Some((first, a)),
            (Some((x, y)), false) if x == a && y == a => None,
            _ => Some((a, a)),
        };
        cx.notify();
    }

    /// Scrolling up pauses; reaching the end again follows.
    fn wheel(&mut self, e: &ScrollEvent, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let dy = f32::from(e.delta.pixel_delta(px(ROW)).y);
        if dy > 0.0 {
            if self.follow {
                self.follow = false;
                cx.notify();
            }
            return;
        }
        cx.on_next_frame(window, |v, _, cx| {
            if !v.follow && v.scroll.is_scrolled_to_end() == Some(true) {
                v.follow = true;
                cx.notify();
            }
        });
    }
}
