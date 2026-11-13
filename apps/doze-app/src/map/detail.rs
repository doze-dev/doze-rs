//! The find box and the selected card's detail: the two overlays that hold words about cards.

use doze_map::{Card, CardId, Mode, Role};
use gpui::{
    AnyElement, BoxShadow, Context, Entity, MouseButton, SharedString, div, point, prelude::*, px,
};
use gpui_component::input::{Input, InputState};

use super::MapView;
use super::cards::state_dot;
use super::chrome::{corner, enter};
use crate::theme::{Motion, Tokens, alpha};
use crate::widgets::{icon, w};

/// The find box and its hits, at the drawing's top left.
pub(crate) fn search_box(
    t: &Tokens,
    m: &MapView,
    input: &Entity<InputState>,
    cx: &mut Context<MapView>,
) -> AnyElement {
    let mut list = div().flex().flex_col().mt(px(4.));
    for (k, &id) in m.hits.iter().enumerate() {
        let Some(c) = m.layout.card(id) else { continue };
        let what = match (c.role, c.members) {
            (_, n) if n > 1 => format!("{n} instances"),
            (Role::Shared, _) => "shared".to_string(),
            (Role::Lane, _) => "lane".to_string(),
            _ => "calls".to_string(),
        };
        list = list.child(
            div()
                .id(("hit", k))
                .flex()
                .gap(px(8.))
                .px(px(8.))
                .h(px(26.))
                .items_center()
                .rounded(px(6.))
                .when(k == m.hit, |d| d.bg(t.sel))
                .hover(|s| s.bg(t.sel))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.jump_to(id, cx)))
                .child(state_dot(t, px(7.), m.rows.get(&c.name)))
                .child(
                    div()
                        .text_color(t.bright)
                        .child(SharedString::from(c.name.clone())),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_color(t.dim)
                        .child(SharedString::from(what)),
                ),
        );
    }
    let q = input.read(cx).value().to_string();
    if m.hits.is_empty() && !q.is_empty() {
        list = list.child(
            div()
                .px(px(8.))
                .h(px(26.))
                .flex()
                .items_center()
                .text_color(t.dim)
                .child("no card by that name"),
        );
    }
    let panel = div()
        .id("map-find")
        .occlude()
        .absolute()
        .left(px(12.))
        .top(px(12.))
        .w(px(320.))
        .p(px(6.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_size(px(12.5))
        .shadow(vec![shadow(t)])
        .child(Input::new(input))
        .child(list);
    enter(panel, "map-find-in", Motion::of(cx), 0.0, -1.0)
}

fn shadow(t: &Tokens) -> BoxShadow {
    BoxShadow {
        color: alpha(gpui::black(), if t.dark { 0.35 } else { 0.12 }),
        offset: point(px(0.), px(10.)),
        blur_radius: px(30.),
        spread_radius: px(0.),
        inset: false,
    }
}

/// What the layout knows about a card, as rows of the detail card.
fn detail_rows(m: &MapView, c: &Card) -> Vec<(&'static str, String)> {
    let names = |ids: &[CardId]| {
        let mut v: Vec<String> = ids
            .iter()
            .take(6)
            .filter_map(|i| m.layout.card(*i).map(|c| c.name.clone()))
            .collect();
        if ids.len() > 6 {
            v.push(format!("+{} more", ids.len() - 6));
        }
        if v.is_empty() {
            "nothing".to_string()
        } else {
            v.join(", ")
        }
    };
    if c.role == Role::Shared {
        return vec![("used by", names(&c.uses))];
    }
    let mut rows = vec![
        ("calls", names(&c.calls)),
        ("needed by", names(&c.callers)),
        ("uses", names(&c.uses)),
    ];
    if let Some(r) = m.rows.get(&c.name).filter(|r| !r.because.is_empty()) {
        rows.push(("now", r.because.clone()));
    }
    rows
}

/// The detail card's first line: the dot, the name, what it is, and the close button.
fn detail_head(t: &Tokens, m: &MapView, c: &Card, cx: &mut Context<MapView>) -> impl IntoElement {
    let r = m.rows.get(&c.name);
    let mut head = Vec::new();
    head.extend(c.lang.map(|l| l.badge().to_string()));
    if c.members > 1 {
        head.push(format!("{} instances, collapsed", c.members));
    }
    head.extend(r.map(|r| r.look.word().to_string()));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .child(state_dot(t, px(8.), r))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(w(600.))
                .text_color(t.bright)
                .child(SharedString::from(c.name.clone())),
        )
        .child(
            div()
                .text_color(t.dim)
                .child(SharedString::from(head.join(" · "))),
        )
        .child(div().flex_1())
        .child(
            div()
                .id("detail-close")
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .hover(|s| s.bg(t.sel))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.select(None, cx)))
                .child(icon("icons/close.svg", 12., t.dim)),
        )
}

/// Where the detail card goes: on a small map, the place the layout keeps clear of the cards (Go's flow.go); on a
/// large one, the corner the card is not in.
fn detail_at(m: &MapView, width: f32, height: f32) -> (f32, f32) {
    let (gx, gy) = m.cam.to_screen(m.layout.detail.x, m.layout.detail.y);
    let fits = m.layout.mode == Mode::Classic
        && gx >= 0.
        && gy >= 0.
        && gx + width <= m.cam.size.0
        && gy + height <= m.cam.size.1;
    if fits {
        (gx, gy)
    } else {
        corner(m, (width, height), true, 12.)
    }
}

/// The selected card's detail.
pub(crate) fn detail(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> Option<AnyElement> {
    let c = m.layout.card(m.lit?)?;
    let top = detail_head(t, m, c, cx);
    let width = 420.;
    let rows = detail_rows(m, c);
    let height = 44. + 21. * rows.len() as f32;
    let (x, y) = detail_at(m, width, height);
    let mut card = div()
        .id("map-detail")
        .occlude()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(width))
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(14.))
        .py(px(9.))
        .rounded(px(11.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_size(px(12.5))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .shadow(vec![shadow(t)])
        .child(top);
    for (k, v) in rows {
        card = card.child(
            div()
                .flex()
                .gap(px(8.))
                .line_height(px(21.))
                .child(div().w(px(76.)).flex_none().text_color(t.dim).child(k))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(t.text)
                        .child(SharedString::from(v)),
                ),
        );
    }
    Some(enter(
        card,
        SharedString::from(format!("detail-{}", c.name)),
        Motion::of(cx),
        1.0,
        0.0,
    ))
}
