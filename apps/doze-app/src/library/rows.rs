//! The Library's table (`.lib`, `.lrow`): a row a stack, its dot, name and folder, its figures, its addresses, when it
//! was last up, its switch and its menu.

use doze_desk::library::LibStack;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::LibraryView;
use crate::kit::{heading, need, pill, s, tone};
use crate::theme::{Tokens, ty};
use crate::widgets::{MONO_FAMILY, dot, icon, switch};

/// The columns' widths: the dot, then name / running / addresses share the rest, then last and the controls.
const DOT_W: f32 = 10.;
const WHEN_W: f32 = 96.;
const ACT_W: f32 = 86.;

fn cols<E: ParentElement>(row: E, cells: [AnyElement; 6]) -> E {
    let [d, name, fig, pills, when, act] = cells;
    row.child(div().w(px(DOT_W)).flex_none().child(d))
        .child(div().flex_1().min_w(px(150.)).child(name))
        .child(div().flex_1().min_w(px(200.)).child(fig))
        .child(div().flex_1().min_w(px(120.)).child(pills))
        .child(div().w(px(WHEN_W)).flex_none().child(when))
        .child(div().w(px(ACT_W)).flex_none().child(act))
}

/// The whole table.
pub(super) fn table(
    t: &Tokens,
    v: &LibraryView,
    cx: &mut Context<LibraryView>,
) -> impl IntoElement {
    let head = |text: &str| heading(text, t.dim, ty::SEMIBOLD, 0.55).into_any_element();
    let header = cols(
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(16.))
            .pl(px(14.))
            .py(px(7.))
            .bg(t.well)
            .rounded_t(px(10.)),
        [
            div().into_any_element(),
            head("Stack"),
            head("Running"),
            head("Addresses"),
            div()
                .flex()
                .justify_end()
                .child(head("Last"))
                .into_any_element(),
            div().into_any_element(),
        ],
    );
    let mut table = div()
        .flex()
        .flex_col()
        .flex_none()
        .mx(px(28.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(header);
    let n = v.lib.stacks.len();
    for (i, st) in v.lib.stacks.iter().enumerate() {
        table = table.child(row(t, v, st, i + 1 == n, cx));
    }
    table
}

fn row(
    t: &Tokens,
    v: &LibraryView,
    st: &LibStack,
    last: bool,
    cx: &mut Context<LibraryView>,
) -> impl IntoElement {
    let selected = v.selected.as_deref() == Some(st.key.as_str());
    let busy = super::act::busy(&st.key, cx);
    let menu = v.menu.as_deref() == Some(st.key.as_str());
    let pad = v.density.pad();
    let key = st.key.clone();
    let (k1, k2) = (key.clone(), key.clone());
    let dot_c = match st.dot {
        "g" => t.good,
        "r" => t.bad,
        "y" => t.warn,
        _ => t.dim,
    };
    let base = div()
        .id(gpui::SharedString::from(format!("lrow-{key}")))
        .relative()
        .flex()
        .items_center()
        .gap(px(14.))
        .pl(px(14.))
        .pr(px(16.))
        .py(px(pad))
        .border_t_1()
        .border_color(t.line)
        .when(last, |d| d.rounded_b(px(10.)))
        .when(selected, |d| d.bg(t.pick))
        .when(!selected, |d| d.hover(|d| d.bg(t.sel)))
        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
            if e.click_count() >= 2 {
                this.open(&k1, cx);
            } else {
                this.select(&k1, cx);
            }
        }));
    let cells = [
        dot(dot_c).into_any_element(),
        name(t, st, v.density.pad() < 8.).into_any_element(),
        fig(t, st).into_any_element(),
        pills(t, st).into_any_element(),
        div()
            .flex()
            .justify_end()
            .text_size(px(ty::SM))
            .text_color(if busy.is_some() { t.cool } else { t.dim })
            .whitespace_nowrap()
            .child(busy.map_or_else(|| st.when.clone(), |b| format!("{}…", b.word())))
            .into_any_element(),
        act(t, st, (menu, busy), &k2, cx).into_any_element(),
    ];
    let mut r = cols(base, cells);
    if menu {
        r = r.child(super::menu::menu(t, st, cx));
    }
    r
}

fn name(t: &Tokens, st: &LibStack, tight: bool) -> impl IntoElement {
    let dir = if st.dir.is_empty() {
        div().child("run by an older doze")
    } else {
        div()
            .flex()
            .min_w_0()
            .child(
                // The folder's end shows, where the stack's own name is: cut at its start with "…" where the column
                // ends (Go's `.lrow .dir .p`, drawn right to left), home already written as ~.
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .child(s(&st.dir)),
            )
            .child(
                div()
                    .flex_none()
                    .child(s(&format!(" · {} declared", st.declared))),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap(px(if tight { 0. } else { 2. }))
        .min_w_0()
        .child(
            div()
                .text_size(px(ty::MD))
                .font_weight(ty::HEADING)
                .text_color(if st.running { t.bright } else { t.text })
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&st.name)),
        )
        .child(
            dir.font_family(MONO_FAMILY)
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .whitespace_nowrap()
                .overflow_hidden(),
        )
}

fn fig(t: &Tokens, st: &LibStack) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .min_w_0()
        .text_size(px(ty::SM))
        .whitespace_nowrap()
        .child(
            div()
                .overflow_hidden()
                .text_ellipsis()
                .child(bits(t, &st.fig)),
        )
        .child(
            div()
                .text_size(px(ty::XS))
                .text_color(t.dim)
                .overflow_hidden()
                .text_ellipsis()
                .child(s(&st.sub)),
        )
}

fn pills(t: &Tokens, st: &LibStack) -> impl IntoElement {
    let mut p = div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .min_w_0()
        .max_h(px(50.))
        .overflow_hidden();
    let red = [&st.missing, &st.broken, &st.need]
        .into_iter()
        .find(|x| !x.is_empty());
    if let Some(r) = red {
        p = p.child(need(t, r));
    }
    for a in &st.addresses {
        p = p.child(pill(t, &a.text, a.up, a.up && !a.url.is_empty()));
    }
    if st.more > 0 {
        p = p.child(pill(t, &format!("+{}", st.more), false, false));
    }
    p
}

fn act(
    t: &Tokens,
    st: &LibStack,
    (menu, busy): (bool, Option<doze_desk::act::Busy>),
    key: &str,
    cx: &mut Context<LibraryView>,
) -> impl IntoElement {
    let k = key.to_owned();
    let k_sw = key.to_owned();
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(4.))
        .child(
            div()
                .id(gpui::SharedString::from(format!("sw-{key}")))
                .cursor_pointer()
                .child(switch(
                    t,
                    busy.map_or(st.running, doze_desk::act::Busy::heading_up),
                    true,
                    busy.is_some()
                        || !st.missing.is_empty()
                        || (!st.running && !st.broken.is_empty()),
                ))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&k_sw, cx))),
        )
        .child(
            div()
                .id(gpui::SharedString::from(format!("more-{key}")))
                .size(px(28.))
                .rounded(px(7.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(menu, |d| d.bg(t.sel))
                .hover(|d| d.bg(t.sel))
                .child(icon(
                    "icons/dots.svg",
                    16.,
                    if menu { t.accent } else { t.dim },
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = if this.menu.as_deref() == Some(k.as_str()) {
                        None
                    } else {
                        Some(k.clone())
                    };
                    cx.notify();
                })),
        )
}

/// Toned runs as one text, so the line is cut with an ellipsis where the column ends.
fn bits(t: &Tokens, fig: &[doze_desk::library::Bit]) -> gpui::StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    for b in fig {
        let start = text.len();
        text.push_str(&b.text);
        let style = gpui::HighlightStyle {
            color: Some(tone(t, b.tone)),
            ..Default::default()
        };
        runs.push((start..text.len(), style));
    }
    gpui::StyledText::new(text).with_highlights(runs)
}
