//! The overview's right column: the vitals charts, or why there are none, and the module's blocks after them.

use doze_desk::page::instance::InstancePage;
use doze_desk::page::parts::Chart;
use doze_display::Look;
use gpui::{AnyElement, Hsla, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::chart::AreaChart;

use super::extends;
use crate::instance::ui::{card, heading, text};
use crate::theme::{Tokens, alpha, ty};

/// The charts on the right, or why there are none.
pub(super) fn charts(t: &Tokens, p: &InstancePage) -> Vec<AnyElement> {
    let Some(v) = &p.vitals else {
        let why = if p.look == Some(Look::Off) || p.paused {
            "Off: nothing runs, nothing is measured."
        } else {
            "no readings yet"
        };
        return vec![
            card(t)
                .child(heading(t, "Vitals", None))
                .child(div().text_color(t.dim).child(why))
                .into_any_element(),
        ];
    };
    let mut out = vec![
        chart_card(t, "Memory", "ram", &v.ram, t.cool, 90.),
        chart_card(t, "CPU", "cpu", &v.cpu, t.accent, 90.),
        chart_card(t, "Connections", "conns", &v.conns, t.good, 70.),
    ];
    out.extend(extends(t, p, "vitals"));
    out
}

/// One vital: the latest reading as the figure, and an area of the last minutes with a hover that reads any moment.
fn chart_card(
    t: &Tokens,
    title: &str,
    id: &'static str,
    c: &Chart,
    color: Hsla,
    height: f32,
) -> AnyElement {
    let n = c.points.len();
    let label: fn(f64) -> String = match id {
        "ram" => doze_display::bytes_label,
        "cpu" => |v| format!("{} cores", doze_display::cores_unit(v)),
        _ => doze_display::count,
    };
    let data: Vec<(usize, f64)> = c.points.iter().copied().enumerate().collect();
    let area_el = if n > 1 {
        AreaChart::new(data)
            .id(id)
            .x(move |(i, _)| SharedString::from(ago(n - 1 - i)))
            .y(|(_, v)| *v)
            .stroke(gpui::transparent_black())
            .fill(gpui::transparent_black())
            .linear()
            .x_axis(false)
            .grid(false)
            .y_padding(3., 3.)
            .y_domain(c.lo, c.hi.max(c.lo + 1.0))
            .name(title.to_owned())
            .tooltip_value(move |_, _, v| SharedString::from(label(v)))
            .into_any_element()
    } else {
        div()
            .text_size(px(ty::XS))
            .text_color(t.dim)
            .child("no readings yet")
            .into_any_element()
    };
    card(t)
        .child(heading(t, &format!("{title} · last {}", c.span), None))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(ty::FIGURE))
                        .font_weight(ty::SEMIBOLD)
                        .text_color(t.bright)
                        .child(text(&c.now)),
                )
                .child(div().text_color(t.dim).child(text(&c.sub))),
        )
        .child(
            div()
                .relative()
                .h(px(height))
                .w_full()
                .when(n > 1, |d| {
                    d.child(area(
                        std::rc::Rc::new(c.points.clone()),
                        c.lo,
                        c.hi.max(c.lo + 1.0),
                        color,
                    ))
                })
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .size_full()
                        .child(area_el),
                ),
        )
        .into_any_element()
}

/// How long before now reading `back` was taken, a second apart: `now`, `12s ago`, `3m 05s ago`.
fn ago(back: usize) -> String {
    match back {
        0 => "now".to_owned(),
        s if s < 60 => format!("{s}s ago"),
        s => format!("{}m {:02}s ago", s / 60, s % 60),
    }
}

/// Go's chart's drawing width: its SVG's viewBox is 400 wide, fitted (`meet`) and centred in the card, so a card wider
/// than that has the chart inset from both sides, and a narrower one scales it down.
const VIEW_W: f32 = 400.;

/// The area as Go's chart drew it: a 2px line over a fill of the same colour, a little in from the top and bottom.
/// (gpui-component's area chart strokes 1px, too faint in the light themes; it is laid over this for its hover.)
fn area(points: std::rc::Rc<Vec<f64>>, lo: f64, hi: f64, color: Hsla) -> AnyElement {
    gpui::canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let (bw, bh) = (f32::from(b.size.width), f32::from(b.size.height));
            let scale = (bw / VIEW_W).min(1.);
            let (w, h) = (VIEW_W * scale, bh * scale);
            let (ox, oy) = ((bw - w) / 2., (bh - h) / 2.);
            let span = (hi - lo).max(f64::EPSILON);
            let last = points.len().saturating_sub(1).max(1) as f32;
            let at = |i: usize, v: f64| {
                let x = ox + i as f32 / last * w;
                let y = oy + h - ((v - lo) / span) as f32 * (h - 6. * scale) - 3. * scale;
                gpui::point(b.origin.x + px(x), b.origin.y + px(y))
            };
            let floor = b.origin.y + px(oy + h);
            let line: Vec<gpui::Point<gpui::Pixels>> =
                points.iter().enumerate().map(|(i, v)| at(i, *v)).collect();
            let (Some(first), Some(end)) = (line.first().copied(), line.last().copied()) else {
                return;
            };
            let mut fill = gpui::PathBuilder::fill();
            let mut poly = line.clone();
            poly.push(gpui::point(end.x, floor));
            poly.push(gpui::point(first.x, floor));
            fill.add_polygon(&poly, true);
            if let Ok(p) = fill.build() {
                window.paint_path(p, alpha(color, 0.12));
            }
            let mut stroke = gpui::PathBuilder::stroke(px(2. * scale));
            stroke.add_polygon(&line, false);
            if let Ok(p) = stroke.build() {
                window.paint_path(p, color);
            }
        },
    )
    .absolute()
    .top(px(0.))
    .left(px(0.))
    .size_full()
    .into_any_element()
}
