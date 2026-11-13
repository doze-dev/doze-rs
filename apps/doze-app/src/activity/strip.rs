//! The strips' painting and the pointer over them (a stretch outlined, its card), the card's head and legend, and
//! What happened.

use gpui::{
    AnyElement, Bounds, Hsla, IntoElement, Pixels, Point, anchored, canvas, deferred, div, fill,
    point, prelude::*, px, size,
};

use super::rows::{COLS, cell};
use super::{ActivityView, head};
use crate::instance::ui::{card, heading, text};
use crate::theme::{Tokens, alpha};
use crate::widgets::w;

/// Each drawn strip's bounds, by row.
pub(super) type Tracks =
    std::rc::Rc<std::cell::RefCell<std::collections::HashMap<usize, Bounds<Pixels>>>>;

/// The strip, painted: the grid, the stretches with their starts, the marks; the one under the pointer outlined. Its
/// bounds are kept for the pointer.
pub(super) fn track(
    t: &Tokens,
    r: &doze_desk::activity::Row,
    ticks: Vec<f64>,
    lit: Option<(usize, bool)>,
    (tracks, row): (Tracks, usize),
) -> AnyElement {
    let t = *t;
    let spans = r.spans.clone();
    let marks = r.marks.clone();
    canvas(
        move |_, _, _| (),
        move |b, _, window, _| {
            tracks.borrow_mut().insert(row, b);
            let x = |f: f64| b.origin.x + b.size.width * (f as f32);
            for k in &ticks {
                window.paint_quad(fill(
                    Bounds::new(point(x(*k), b.origin.y), size(px(1.), b.size.height)),
                    t.line,
                ));
            }
            for (i, s) in spans.iter().enumerate() {
                let width = (b.size.width * ((s.to - s.from).max(0.004) as f32)).max(px(2.));
                let at = Bounds::new(point(x(s.from), b.origin.y + px(7.)), size(width, px(12.)));
                let c = if s.open { t.good } else { alpha(t.good, 0.7) };
                let mut q = fill(at, c).corner_radii(px(4.));
                if lit == Some((i, false)) {
                    q = q.border_widths(px(1.5)).border_color(t.bright);
                }
                window.paint_quad(q);
                if s.ready > s.from {
                    let rw = (b.size.width * ((s.ready - s.from) as f32)).min(width);
                    window.paint_quad(
                        fill(Bounds::new(at.origin, size(rw, px(12.))), t.cool)
                            .corner_radii(px(4.)),
                    );
                }
            }
            for (i, m) in marks.iter().enumerate() {
                let c = if m.kind == "give_up" { t.warn } else { t.bad };
                let wide = if lit == Some((i, true)) { 6. } else { 4. };
                let at = Bounds::new(
                    point(x(m.at) - px(wide / 2.), b.origin.y + px(4.)),
                    size(px(wide), px(18.)),
                );
                window.paint_quad(fill(at, c).corner_radii(px(2.)));
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

/// What is under the pointer on a row's strip: a crash mark (within 4 px) before a stretch.
pub(super) fn hit(
    v: &ActivityView,
    row: usize,
    b: Bounds<Pixels>,
    pos: Point<Pixels>,
) -> Option<(usize, bool)> {
    let r = v.row_at(row)?;
    let wpx = f32::from(b.size.width).max(1.);
    let f = f64::from(f32::from(pos.x - b.origin.x) / wpx);
    if !b.contains(&pos) {
        return None;
    }
    let near = |a: f64| ((a - f).abs() * f64::from(wpx)) <= 4.0;
    if let Some(i) = r.marks.iter().position(|m| near(m.at)) {
        return Some((i, true));
    }
    r.spans
        .iter()
        .position(|s| f >= s.from && f <= s.to.max(s.from + 0.004))
        .map(|i| (i, false))
}

/// The card under the pointer: the stretch's words, one to a line, and how long its start took.
pub(super) fn hover_card(t: &Tokens, v: &ActivityView) -> Option<AnyElement> {
    let h = v.hover.as_ref()?;
    let r = v.row_at(h.row)?;
    let mut lines: Vec<String> = Vec::new();
    if h.mark {
        lines.push(r.marks.get(h.at)?.title.clone());
    } else {
        let s = r.spans.get(h.at)?;
        lines.extend(s.title.split(" · ").map(str::to_owned));
        if let Some(d) = s.took {
            lines.push(format!("ready in {}", took(d)));
        }
    }
    let first = lines.first().cloned().unwrap_or_default();
    let card = div()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(9.))
        .bg(t.bright)
        .text_color(t.bg)
        .text_size(px(12.))
        .line_height(px(18.))
        .whitespace_nowrap()
        .shadow(vec![gpui::BoxShadow {
            color: alpha(gpui::black(), 0.25),
            offset: point(px(0.), px(4.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(
            div()
                .font_weight(w(600.))
                .child(text(&format!("{} · {first}", r.name))),
        )
        .children(lines.iter().skip(1).map(|l| div().child(text(l))));
    Some(
        deferred(
            anchored()
                .position(h.pos + point(px(12.), px(16.)))
                .snap_to_window_with_margin(px(8.))
                .child(card),
        )
        .with_priority(2)
        .into_any_element(),
    )
}

fn took(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

/// The card's head: the column titles, and the axis over the strips.
pub(super) fn card_head(t: &Tokens, m: &doze_desk::activity::Activity) -> AnyElement {
    let title = |s: &str, width: f32, right: bool| {
        cell(t, width, right).child(crate::widgets::tracked(s, 11., 600., t.dim, 0.55))
    };
    let ticks = m.ticks.clone();
    let axis =
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(16.))
            .children(ticks.iter().enumerate().map(|(i, k)| {
                let last = i + 1 == ticks.len();
                div()
                    .absolute()
                    .top(px(0.))
                    .left(gpui::relative(k.at as f32))
                    .when(i > 0 && !last, |d| d.ml(px(-14.)))
                    .when(last, |d| d.ml(px(-24.)))
                    .text_size(px(11.))
                    .text_color(t.dim)
                    .whitespace_nowrap()
                    .child(text(&k.label.to_uppercase()))
            }));
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .mx(px(28.))
        .px(px(16.))
        .pt(px(12.))
        .pb(px(6.))
        .border_t_1()
        .border_l_1()
        .border_r_1()
        .border_color(t.line)
        .rounded_t(px(10.))
        .bg(t.panel)
        .child(title("SERVICE", COLS[0], false))
        .child(axis)
        .child(title("ASLEEP", COLS[1], false))
        .child(title("WAKES", COLS[2], true))
        .child(
            div()
                .flex_1()
                .max_w(px(COLS[3]))
                .child(crate::widgets::tracked("WOKEN BY", 11., 600., t.dim, 0.55)),
        )
        .child(title("FREED", COLS[4], true))
        .into_any_element()
}

/// `.tl-legend`: what the colours mean, and what is not counted.
pub(super) fn legend(t: &Tokens, none_found: bool) -> AnyElement {
    let sw = |c: Hsla, wake: bool| {
        div()
            .w(px(22.))
            .h(px(10.))
            .rounded(px(4.))
            .bg(c)
            .overflow_hidden()
            .when(wake, |d| d.child(div().size_full().bg(t.cool)))
    };
    let item = |e: AnyElement, s: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(e)
            .child(text(s))
    };
    div()
        .flex()
        .flex_col()
        .mx(px(28.))
        .mb(px(14.))
        .px(px(16.))
        .pt(px(6.))
        .pb(px(10.))
        .border_b_1()
        .border_l_1()
        .border_r_1()
        .border_color(t.line)
        .rounded_b(px(10.))
        .bg(t.panel)
        .when(none_found, |d| d.child(div().py(px(8.)).text_color(t.dim).child("no service by that name · esc shows them all")))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(18.))
                .text_size(px(12.))
                .text_color(t.dim)
                .whitespace_nowrap()
                .child(item(sw(alpha(t.good, 0.7), false).into_any_element(), "awake"))
                .child(item(sw(t.good, true).into_any_element(), "starting, up to ready"))
                .child(item(div().w(px(4.)).h(px(12.)).rounded(px(2.)).bg(t.bad).into_any_element(), "crashed"))
                .child(div().flex_1())
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child("a name shows that service alone; time with the stack down is not counted as sleep"),
                ),
        )
        .into_any_element()
}

/// What happened, newest first; or that nothing did.
pub(super) fn events(t: &Tokens, m: &doze_desk::activity::Activity) -> AnyElement {
    let body: AnyElement = if m.empty {
        div()
            .text_color(t.dim)
            .child("Nothing woke, slept or crashed in this span.")
            .into_any_element()
    } else {
        head::lines(t, &m.events).into_any_element()
    };
    div()
        .px(px(28.))
        .pb(px(18.))
        .child(
            card(t)
                .child(heading(t, "WHAT HAPPENED", Some("· newest first")))
                .child(body),
        )
        .into_any_element()
}
