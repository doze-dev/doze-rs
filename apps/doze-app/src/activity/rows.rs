//! The page below the head, as one virtualised list: the summary, Trouble, the card of strips (its head, a row per
//! service, its legend), What happened. Only the items in view are built; each strip is painted on a canvas: its
//! grid, its awake stretches (the start, up to ready, in the cool colour), its crash marks, and the stretch under the
//! pointer outlined.

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, Hsla, IntoElement, MouseMoveEvent,
    Window, div, prelude::*, px,
};

use super::{ActivityView, head, strip};
use crate::instance::ui::text;
use crate::theme::{Motion, Tokens};
use crate::widgets::{dot, w};

/// The list's first service row: after the summary, Trouble and the card's head.
pub(crate) const FIRST_ROW: usize = 3;

/// A service row's height.
const ROW_H: f32 = 32.;

/// How many items the list has for `n` service rows: three before them, the legend and What happened after.
pub(super) fn items(n: usize) -> usize {
    FIRST_ROW + n + 2
}

/// Go's column widths: name, the strip, asleep, wakes, woken by, freed.
pub(super) const COLS: [f32; 5] = [140., 110., 54., 220., 90.];

pub(super) fn list(t: &Tokens, v: &mut ActivityView, cx: &mut Context<ActivityView>) -> AnyElement {
    let t = *t;
    gpui::list(
        v.list.clone(),
        cx.processor(move |v: &mut ActivityView, ix: usize, window, cx| {
            gpui::div()
                .w_full()
                .flex()
                .flex_col()
                .child(v.item(&t, ix, window, cx))
                .into_any_element()
        }),
    )
    .flex_1()
    .min_h_0()
    .into_any_element()
}

impl ActivityView {
    fn item(
        &mut self,
        t: &Tokens,
        ix: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(m) = self.model.as_ref() else {
            return div().into_any_element();
        };
        let n = self.shown.len();
        match ix {
            0 => head::summary(t, m),
            1 => {
                let more: crate::kit::Click =
                    Box::new(cx.listener(|v: &mut ActivityView, _, _, cx| {
                        v.all_trouble = true;
                        v.list.splice(1..2, 1);
                        cx.notify();
                    }));
                head::trouble(t, m, self.all_trouble, more)
            }
            2 => strip::card_head(t, m),
            i if i < FIRST_ROW + n => self.row(t, i - FIRST_ROW, cx),
            i if i == FIRST_ROW + n => strip::legend(t, n == 0 && !self.find.is_empty()),
            _ => strip::events(t, m),
        }
    }

    fn row(&mut self, t: &Tokens, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(r) = self.row_at(i).cloned() else {
            return div().into_any_element();
        };
        let ticks: Vec<f64> = self
            .model
            .as_ref()
            .map(|m| m.ticks.iter().map(|k| k.at).collect())
            .unwrap_or_default();
        let on = self.cur == Some(i);
        let lit = self
            .hover
            .as_ref()
            .filter(|h| h.row == i)
            .map(|h| (h.at, h.mark));
        let name = r.name.clone();
        let alone = self.only == r.name;
        let strip = strip::track(t, &r, ticks, lit, (self.tracks.clone(), i));
        let strip = div()
            .id(("act-track", i))
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(26.))
            .rounded(px(6.))
            .bg(t.well)
            .overflow_hidden()
            .on_mouse_move(
                cx.listener(move |v, e: &MouseMoveEvent, _, cx| v.point(i, e.position, cx)),
            )
            .on_hover(cx.listener(move |v, over: &bool, _, cx| {
                if !*over {
                    v.unpoint(i, cx);
                }
            }))
            .child(strip);
        let strip = fade(strip, self.landed, Motion::of(cx));
        let body = div()
            .id(("act-row", i))
            .flex()
            .items_center()
            .gap(px(14.))
            .h(px(ROW_H))
            .px(px(16.))
            .mx(px(28.))
            .border_l_1()
            .border_r_1()
            .border_color(t.line)
            .bg(if on { t.sel } else { t.panel })
            .child(name_cell(
                t,
                &r,
                alone,
                cx.listener(move |v, _, _, cx| v.set_only(&name, cx)),
            ))
            .child(strip)
            .child(asleep(t, &r))
            .child(cell(t, COLS[2], true).child(if r.wakes == 0 {
                div().text_color(t.dim).child("0")
            } else {
                div().child(r.wakes.to_string())
            }))
            .child(woken(t, &r))
            .child(cell(t, COLS[4], true).child(if r.freed.is_empty() {
                div().text_color(t.dim).child("—")
            } else {
                div().child(text(&r.freed))
            }));
        body.into_any_element()
    }
}

/// The strips fade in over 180 ms when a new span lands, unless motion is reduced.
fn fade(el: gpui::Stateful<gpui::Div>, landed: u64, motion: Motion) -> AnyElement {
    if !motion.moves() || landed == 0 {
        return el.into_any_element();
    }
    el.with_animation(
        ("act-fade", usize::try_from(landed).unwrap_or(0)),
        Animation::new(motion.normal).with_easing(gpui::ease_out_quint()),
        |d, p| d.opacity(0.35 + 0.65 * p),
    )
    .into_any_element()
}

pub(super) fn cell(t: &Tokens, width: f32, right: bool) -> gpui::Div {
    let _ = t;
    div()
        .w(px(width))
        .flex_none()
        .flex()
        .when(right, |d| d.justify_end())
        .whitespace_nowrap()
        .overflow_hidden()
}

fn name_cell(
    t: &Tokens,
    r: &doze_desk::activity::Row,
    alone: bool,
    click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(gpui::SharedString::from(format!("act-name-{}", r.name)))
        .w(px(COLS[0]))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(26.))
        .text_color(if alone { t.bright } else { t.text })
        .when(alone, |d| d.font_weight(w(600.)))
        .cursor_pointer()
        .hover(|s| s.text_color(t.bright))
        .on_click(click)
        .child(dot(look_color(t, r.look.word)))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(text(&r.name)),
        )
}

/// Go's `dotClass`: awake green, dozing yellow, waking cool, trouble red, the rest dim.
fn look_color(t: &Tokens, word: &str) -> Hsla {
    match word {
        "awake" => t.good,
        "dozing" => t.warn,
        "waking" => t.cool,
        "crashing" | "failed" => t.bad,
        _ => t.line2,
    }
}

/// `.act-asleep`: a bar of the share asleep, and the share (or "never", in the warning colour).
fn asleep(t: &Tokens, r: &doze_desk::activity::Row) -> impl IntoElement {
    #[allow(clippy::cast_possible_truncation)] // WHY: a share to a whole percent
    let pct = (r.asleep_part * 100.0).round() as i64;
    let words = if r.never {
        "never".to_owned()
    } else {
        format!("{pct}%")
    };
    cell(t, COLS[1], false)
        .id(gpui::SharedString::from(format!("act-asleep-{}", r.name)))
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .h(px(6.))
                .rounded(px(3.))
                .bg(t.track)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(r.asleep_part as f32))
                        .bg(t.accent),
                ),
        )
        .child(
            div()
                .text_color(if r.never { t.warn } else { t.text })
                .child(words),
        )
        .tooltip({
            let say = r.asleep.clone();
            move |window, cx| gpui_component::tooltip::Tooltip::new(say.clone()).build(window, cx)
        })
}

/// What woke it, and how often it crashed.
fn woken(t: &Tokens, r: &doze_desk::activity::Row) -> impl IntoElement {
    let by = if r.woken_by.is_empty() {
        "—".to_owned()
    } else {
        r.woken_by.clone()
    };
    div()
        .flex()
        .flex_1()
        .max_w(px(COLS[3]))
        .min_w_0()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_color(t.dim)
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(text(&by)),
        )
        .when(r.crashes > 0, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_color(t.bad)
                    .child(text(&format!(" · {} crashed", r.crashes))),
            )
        })
}
