//! A tile's foot (Go `Tiles`' `.ft`): its memory and its address, which opens the service in the browser, with the
//! copy mark beside it; or, in trouble, the last thing it said.

use doze_desk::overview::Row;
use gpui::{Context, IntoElement, div, prelude::*, px};

use super::OverviewView;
use crate::kit::s;
use crate::outward::Probe as _;
use crate::theme::{Tokens, ty};
use crate::widgets::{Mono as _, icon};

/// The foot: memory and the address with its copy mark; or, in trouble, what it last said.
pub(super) fn tile_foot(t: &Tokens, r: &Row, cx: &mut Context<OverviewView>) -> gpui::Div {
    if !r.err.is_empty() {
        return div()
            .mt_auto()
            .mono_face()
            .text_size(px(ty::XS_UP))
            .text_color(t.bad)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .child(s(&r.err));
    }
    let f = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .mt_auto()
        .text_size(px(ty::SM_UP))
        .text_color(t.dim)
        .whitespace_nowrap()
        .min_w_0()
        .when(!r.mem.is_empty(), |d| {
            d.child(
                div()
                    .flex_none()
                    .text_color(t.text)
                    .font_features(ty::tabular())
                    .child(s(&r.mem)),
            )
        });
    if r.kind == "task" {
        return f.when(r.engine != "task", |d| {
            d.child(div().mono_face().child(s(&r.engine)))
        });
    }
    f.when(!r.address.is_empty(), |d| {
        d.child(address(t, r, cx)).child(copy_mark(t, r, cx))
    })
}

/// The address (Go `Address`): a button that opens the row's URL whenever it has one, awake or not (a browser's
/// connection through doze is what wakes it); drawn as a link only while it is awake.
pub(super) fn address(t: &Tokens, r: &Row, cx: &mut Context<OverviewView>) -> impl IntoElement {
    let link = r.awake && !r.url.is_empty();
    let url = r.url.clone();
    div()
        .id(gpui::SharedString::from(format!("addr-{}", r.name)))
        .flex_initial()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        // Go's button takes the foot's face and colour (`font: inherit; color: inherit`); a link is underlined in the
        // waking blue, faintly (`.link`'s decoration; the inline colour wins over its text colour).
        .when(link, |d| {
            d.underline()
                .text_decoration_color(crate::theme::alpha(t.cool, 0.4))
        })
        .when(!url.is_empty(), |d| d.cursor_pointer())
        .child(s(&r.address))
        .probe(format!("addr-{}", r.name), r.address.clone())
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            if !url.is_empty() {
                crate::outward::open_url(cx, &url);
            }
        }))
}

/// The copy mark beside an address: the URL (or the address), and the toast that says so (Go `copy`).
pub(super) fn copy_mark(t: &Tokens, r: &Row, cx: &mut Context<OverviewView>) -> impl IntoElement {
    let (name, what) = (
        r.name.clone(),
        if r.url.is_empty() {
            r.address.clone()
        } else {
            r.url.clone()
        },
    );
    div()
        .id(gpui::SharedString::from(format!("copy-{}", r.name)))
        .flex_none()
        .p(px(4.))
        .m(px(-4.))
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|d| d.bg(t.sel))
        .child(icon("icons/copy.svg", 12., t.accent))
        .probe(
            format!("copy-{}", r.name),
            format!("Copy the address of {}", r.name),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.copy_text(&what, &format!("the address of {name}"), cx);
        }))
}
