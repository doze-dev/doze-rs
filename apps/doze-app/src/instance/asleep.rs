//! The asleep module's page, and a service's web console tab.

use doze_display::Look;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::InstanceView;
use super::overview::connect;
use super::ui::{button, text};
use crate::theme::{Tokens, ty};
use crate::widgets::MONO_FAMILY;

/// An asleep module or service: it wakes on the next connection; Wake, or wake and open its console.
pub(crate) fn asleep(
    t: &Tokens,
    v: &mut InstanceView,
    cx: &mut Context<InstanceView>,
) -> AnyElement {
    let p = &v.page;
    let addr = p
        .connect
        .iter()
        .find(|r| r.k == "URL" || r.k == "Address")
        .map(|r| r.v.clone());
    let to = addr.map(|a| format!(" to {a}")).unwrap_or_default();
    let mut acts = div().flex().gap(px(10.)).mt(px(8.));
    if p.web.is_empty() {
        acts = acts.child(
            button(t, "wake", "Wake", &super::head::chip("Wake"), true)
                .on_click(cx.listener(|v, _, w, cx| v.flip(w, cx))),
        );
    } else {
        acts = acts
            .child(
                button(t, "wake-open", "Wake and open the console", "", true).on_click(
                    cx.listener(|v, _, w, cx| {
                        v.flip(w, cx);
                        v.set_tab(doze_display::TAB_WEB, w, cx);
                    }),
                ),
            )
            .child(
                button(t, "wake", "Just wake", "", false)
                    .on_click(cx.listener(|v, _, w, cx| v.flip(w, cx))),
            );
    }
    let mut page = div()
        .id("asleep")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .px(px(40.))
                .pt(px(36.))
                .pb(px(20.))
                .child(div().text_size(px(ty::LG_UP)).font_weight(ty::HEADING).text_color(t.bright).child(text(&format!("{} is asleep.", v.name))))
                .child(div().max_w(px(520.)).text_color(t.dim).text_center().child(text(&format!(
                    "It wakes on the next connection{to}, and sleeps again when nothing holds it. Its data stays in between."
                ))))
                .child(acts),
        );
    if !p.connect.is_empty() {
        page = page.child(div().px(px(28.)).pb(px(16.)).child(connect(
            t,
            v,
            "Connect",
            Some("· works while asleep: the first connection wakes it"),
            cx,
        )));
    }
    page.into_any_element()
}

/// A service's own web console. GPUI has no web view (PLAN.md Q1, option a): the tab says what it is, wakes it on
/// the way, and opens it in the browser.
pub(crate) fn web_console(
    t: &Tokens,
    v: &mut InstanceView,
    cx: &mut Context<InstanceView>,
) -> AnyElement {
    let url = v.page.web.clone();
    let asleep = v.page.look == Some(Look::Asleep);
    let top = web_bar(t, v, &url, asleep, cx);
    let say = if url.is_empty() {
        format!("{} says it has a console but gave no address.", v.name)
    } else if asleep {
        "Opening it wakes it: the first connection does. The console runs in your browser; doze's window has no web view.".to_owned()
    } else {
        "The console runs in your browser; doze's window has no web view.".to_owned()
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .mx(px(28.))
        .my(px(16.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .overflow_hidden()
        .child(top)
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p(px(40.))
                .text_color(t.dim)
                .child(text(&say)),
        )
        .into_any_element()
}

/// The web console's bar: the name and address, Copy address, and Open in browser (which wakes it if asleep).
fn web_bar(
    t: &Tokens,
    v: &InstanceView,
    url: &str,
    asleep: bool,
    cx: &mut Context<InstanceView>,
) -> gpui::Div {
    let copy = v
        .page
        .connect
        .iter()
        .find(|r| !r.copy.is_empty())
        .map(|r| r.copy.clone());
    let mut top = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(44.))
        .px(px(16.))
        .border_b_1()
        .border_color(t.line)
        .text_color(t.bright)
        .font_weight(ty::SEMIBOLD)
        .child(text(&v.name))
        .child(
            div()
                .font_weight(ty::REGULAR)
                .font_family(MONO_FAMILY)
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(text(&format!("· {url}"))),
        )
        .child(div().flex_1());
    if let Some(c) = copy {
        top = top.child(
            button(
                t,
                "copy-addr",
                if v.is_copied("web") {
                    "Copied"
                } else {
                    "Copy address"
                },
                &super::head::chip("Copy address"),
                false,
            )
            .on_click(cx.listener(move |v, _, _, cx| v.copy("web", &c, cx))),
        );
    }
    let u2 = url.to_owned();
    top.child(
        button(
            t,
            "open-web",
            "Open in browser",
            &super::head::chip("Open in browser"),
            true,
        )
        .on_click(cx.listener(move |v, _, w, cx| {
            if asleep {
                v.flip(w, cx);
            }
            cx.open_url(&u2);
        })),
    )
}
