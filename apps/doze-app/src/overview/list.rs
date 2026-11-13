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
use crate::theme::{Density, Tokens};
use crate::widgets::{tracked, w};

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
        .child(tracked(&title, 11., 700., t.dim, 1.1))
        .into_any_element()
}

/// The word in its tone, why, and what it last said while in trouble.
fn why(t: &Tokens, r: &Row) -> gpui::Div {
    let word = div()
        .flex_none()
        .font_weight(w(600.))
        .text_color(word_tone(t, r))
        .child(r.look.word);
    let because = div().flex_none().child(s(&format!(" · {}", r.because)));
    let err = div()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .child(s(&format!(" · {}", r.err)));
    div()
        .flex()
        .flex_1()
        .min_w(px(170.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(if r.trouble { t.bad } else { t.dim })
        .child(word)
        .when(!r.because.is_empty(), |d| d.child(because))
        .when(!r.err.is_empty(), |d| d.child(err))
}

/// `.qr.narrow`: dot, name, the word and why, memory, the switch.
fn row(
    t: &Tokens,
    r: &Row,
    selected: bool,
    density: Density,
    this: &WeakEntity<OverviewView>,
    cx: &mut App,
) -> AnyElement {
    let off = !r.on && r.kind != "task";
    let (name, weak) = (r.name.clone(), this.clone());
    let switch = this
        .update(cx, |_, cx| row_switch(t, r, cx))
        .unwrap_or_else(|_| div().into_any_element());
    let title = div()
        .flex_1()
        .min_w(px(110.))
        .text_size(px(14.))
        .font_weight(w(600.))
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
        .text_size(px(12.5))
        .text_color(t.text)
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
        .when(selected, |d| d.bg(t.pick))
        .when(!selected, |d| d.hover(|d| d.bg(t.sel)))
        .child(div().w(px(10.)).flex_none().child(row_dot(t, r)))
        .child(title)
        .child(why(t, r).flex_grow(2.))
        .child(mem)
        .child(
            div()
                .w(px(34.))
                .flex_none()
                .flex()
                .justify_end()
                .child(switch),
        )
        .on_click(move |_, _, cx| {
            let _ = weak.update(cx, |this, cx| this.select(&name, cx));
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
    let rows = list(v.list.clone(), move |ix, _, cx| match data.get(ix) {
        Some(Item::Head(title)) => head(&t2, title),
        Some(Item::Row(r)) => row(
            &t2,
            r,
            sel.as_deref() == Some(r.name.as_str()),
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
        .child(filter(t, v))
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
        .child(super::pane::pane(t, v, cx))
}
