//! The List view (the List board): the services as rows under their groups, a virtual list (gpui's `list`: only the
//! rows in view are built, so a stack of two thousand scrolls as one of ten), with the selected one in full in the
//! pane beside it. Row height follows the density.

use std::rc::Rc;

use doze_desk::overview::Row;
use gpui::{
    AnyElement, App, Context, IntoElement, ListAlignment, ListState, WeakEntity, div, list,
    prelude::*, px,
};

use super::OverviewView;
use super::tiles::{filter, row_dot, row_switch, word_tone};
use crate::kit::s;
use crate::theme::{Density, Tokens, ty};
use crate::widgets::tracked;

/// One line of the list.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Item {
    Head(&'static str),
    Row(Box<Row>),
}

/// The list's lines: each group's heading, then its rows the filter leaves.
pub(super) fn items(v: &OverviewView) -> Vec<Item> {
    let shown: Vec<&str> = v.shown().iter().map(|r| r.name.as_str()).collect();
    let mut out = Vec::new();
    for g in &v.home.groups {
        let rows: Vec<&Row> = g
            .rows
            .iter()
            .filter(|r| shown.contains(&r.name.as_str()))
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push(Item::Head(g.title));
        out.extend(rows.into_iter().map(|r| Item::Row(Box::new(r.clone()))));
    }
    out
}

fn head(t: &Tokens, title: &str) -> AnyElement {
    let title = format!(
        "{}{}",
        title.get(..1).unwrap_or(""),
        title.get(1..).unwrap_or("").to_lowercase()
    );
    div()
        .w_full()
        .pt(px(18.))
        .px(px(14.))
        .pb(px(8.))
        .child(tracked(&title, ty::XS, ty::BOLD, t.dim, 1.1))
        .into_any_element()
}

/// Below this window width the list drops its memory column: the name, the state word and the switch come first.
const MEMORY_FROM: f32 = 900.;

/// The word in its tone, why, and what it last said while in trouble.
fn why(t: &Tokens, r: &Row) -> gpui::Div {
    // What it last said, unless that is already why (a failed one's reason is its error: once is enough).
    let err = if r.err == r.because {
        ""
    } else {
        r.err.as_str()
    };
    let rest: Vec<&str> = [r.because.as_str(), err]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect();
    // The word is whole, always ("awake", "waking", "asleep"): only why after it gives way, cut with an ellipsis, so
    // in a narrow window the switch stays in its column under Sleep all and the state still reads.
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .whitespace_nowrap()
        .text_color(if r.trouble { t.bad } else { t.dim })
        .child(
            div()
                .flex_none()
                .font_weight(ty::SEMIBOLD)
                .text_color(word_tone(t, r))
                .child(s(r.look.word)),
        )
        .when(!rest.is_empty(), |d| {
            d.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(s(&format!(" · {}", rest.join(" · ")))),
            )
        })
}

/// `.qr.narrow`: dot, name, the word and why, memory, the switch.
fn row(
    t: &Tokens,
    r: &Row,
    (selected, keys): (bool, (Option<&gpui::FocusHandle>, bool)),
    density: Density,
    this: &WeakEntity<OverviewView>,
    cx: &mut App,
) -> AnyElement {
    let off = !r.on && r.kind != "task";
    let (name, weak) = (r.name.clone(), this.clone());
    let (switch, narrow) = this
        .update(cx, |this, cx| {
            let turning = this.turning.contains(&r.name);
            (
                row_switch(t, r, (&this.home.key, turning), cx),
                this.width < MEMORY_FROM,
            )
        })
        .unwrap_or_else(|_| (div().into_any_element(), false));
    let title = div()
        .flex_1()
        .min_w(px(110.))
        .text_size(px(ty::MD))
        .font_weight(ty::SEMIBOLD)
        .text_color(if off { t.dim } else { t.bright })
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(s(&r.name));
    let mem = div()
        .w(px(66.))
        .flex_none()
        .flex()
        .justify_end()
        .text_size(px(ty::SM_UP))
        .text_color(t.text)
        .font_features(ty::tabular())
        .child(s(&r.mem));
    div()
        .id(gpui::SharedString::from(format!("qr-{}", r.name)))
        .w_full()
        .flex()
        .items_center()
        .gap(px(16.))
        .h(px(density.row()))
        .px(px(14.))
        .rounded(px(if density == Density::Compact { 6. } else { 9. }))
        // Selected: `sel` and the accent bar at the left edge (not a `pick` block); hovered: `sel`.
        .relative()
        .when(selected, |d| {
            d.bg(t.sel).child(crate::widgets::sel_bar(t, 8.))
        })
        .when(!selected, |d| d.hover(|d| d.bg(t.sel)))
        .child(div().w(px(10.)).flex_none().child(row_dot(t, r)))
        .child(
            super::choose::name_ring(t, title, keys)
                .flex_1()
                .min_w(px(110.)),
        )
        .child(why(t, r).flex_grow(2.))
        .when(!narrow, |d| d.child(mem))
        .child(
            div()
                .w(px(34.))
                .flex_none()
                .flex()
                .justify_end()
                .child(switch),
        )
        .on_click(move |ev, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.select(&name, cx);
                if ev.click_count() > 1 {
                    this.open(&name, None, cx);
                }
            });
        })
        .into_any_element()
}

/// The view: the filter and the rows on the left, the pane on the right.
pub(super) fn view(
    t: &Tokens,
    v: &mut OverviewView,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let items = items(v);
    if items != *v.items {
        let n = items.len();
        v.items = Rc::new(items);
        if v.list.item_count() != n {
            v.list = ListState::new(n, ListAlignment::Top, px(200.));
        }
    }
    let (data, t2, sel, density, weak) = (
        v.items.clone(),
        *t,
        v.selected.clone(),
        v.density,
        cx.entity().downgrade(),
    );
    let (handles, keys) = v.names.clone();
    let rows = list(v.list.clone(), move |ix, _, cx| match data.get(ix) {
        Some(Item::Head(title)) => head(&t2, title),
        Some(Item::Row(r)) => row(
            &t2,
            r,
            (
                sel.as_deref() == Some(r.name.as_str()),
                (
                    handles.get(&r.name),
                    keys.as_deref() == Some(r.name.as_str()),
                ),
            ),
            density,
            &weak,
            cx,
        ),
        None => div().into_any_element(),
    })
    .flex_1()
    .min_h_0();
    let main = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .pt(px(28.))
        .child(filter(t, v, Some(super::tiles::LIST_SWITCH_IN), cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .mx(px(28.))
                .pb(px(12.))
                .child(rows),
        );
    div()
        .flex()
        .flex_1()
        .min_h_0()
        .child(main)
        .children(super::pane::pane(t, v, cx))
}
