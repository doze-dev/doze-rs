//! Activity's head (the name, the stack and the span, Every service, the span switch, Back), its summary of five
//! figures, and Trouble: Go's `.head`, `.act-sum`, `.act-trouble`.

use doze_desk::activity::{Activity, Line, Span};
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::ActivityView;
use crate::instance::ui::{card, heading, text};
use crate::kit::{Btn, btn, tone};
use crate::theme::{Tokens, mix};
use crate::widgets::{MONO_FAMILY, seg, w};

/// `.head`: Activity, "shop · db · the last 24 hours", Every service, the span, Back to the stack.
pub(super) fn head(
    t: &Tokens,
    v: &ActivityView,
    cx: &mut Context<ActivityView>,
) -> impl IntoElement {
    let stack = v.model.as_ref().map_or_else(
        || doze_desk::name_from_key(v.key.dir()),
        |m| m.stack.clone(),
    );
    let mut path = stack.clone();
    if !v.only.is_empty() {
        path.push_str(&format!(" · {}", v.only));
    }
    path.push_str(&format!(" · {}", v.span.phrase()));
    let spans = Span::ALL
        .into_iter()
        .map(|s| {
            let click: crate::widgets::OnClick =
                Box::new(cx.listener(move |v: &mut ActivityView, _, _, cx| v.set_span(s, cx)));
            (
                div().child(s.label()).into_any_element(),
                v.span == s,
                Some(click),
            )
        })
        .collect();
    let mut row = title(t, &path);
    if let Some(input) = &v.input {
        row = row.child(
            div()
                .w(px(200.))
                .flex_none()
                .child(gpui_component::Sizable::small(
                    gpui_component::input::Input::new(input),
                )),
        );
    }
    row.when(!v.only.is_empty(), |d| {
        d.child(btn(
            t,
            "act-every",
            "Every service",
            "",
            Btn::Quiet,
            Some(Box::new(cx.listener(|v: &mut ActivityView, _, _, cx| {
                v.set_only("", cx)
            }))),
        ))
    })
    .child(seg(t, "act-span", 11., spans))
    .child(btn(
        t,
        "act-back",
        &format!("Back to {stack}"),
        "esc",
        Btn::Plain,
        Some(Box::new(
            cx.listener(|_, _, _, cx| cx.emit(super::ActivityEvent::Back)),
        )),
    ))
}

/// `.act-sum`: five figures in their own boxes; the note when the journal is shorter than the span.
pub(super) fn summary(t: &Tokens, a: &Activity) -> AnyElement {
    let s = &a.summary;
    let (up, of) = s.up.split_once(" of ").unwrap_or((&s.up, ""));
    let freed = if s.freed.is_empty() {
        "—".to_owned()
    } else {
        s.freed.replacen("about ", "~", 1)
    };
    let stats = div()
        .flex()
        .gap(px(12.))
        .child(stat(t, &s.asleep, "asleep while the stack was up", false))
        .child(
            div()
                .id("act-freed")
                .flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .child(stat(t, &freed, "memory left free by sleeping", false))
                .when(!s.freed_note.is_empty(), |d| {
                    let note = s.freed_note.clone();
                    d.tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(note.clone()).build(window, cx)
                    })
                }),
        )
        .child(stat(t, &s.wakes.to_string(), "wakes", false))
        .child(stat(
            t,
            &s.crashes.to_string(),
            "crashes and failed starts",
            s.crashes > 0,
        ))
        .child(stat(t, up, &format!("up, of {of}"), false));
    div()
        .flex()
        .flex_col()
        .px(px(28.))
        .pb(px(14.))
        .gap(px(10.))
        .child(stats)
        .when(!a.short.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(12.5))
                    .text_color(t.dim)
                    .child(text(&a.short)),
            )
        })
        .into_any_element()
}

fn stat(t: &Tokens, v: &str, s: &str, bad: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .px(px(16.))
        .py(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .child(
            div()
                .text_size(px(26.))
                .line_height(px(30.))
                .font_weight(w(650.))
                .whitespace_nowrap()
                .text_color(if bad { t.bad } else { t.bright })
                .child(text(v)),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.dim)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(text(s)),
        )
}

/// `.act-trouble`: the crashes, failed starts and give-ups, newest first; nothing when there were none.
/// Past Go: six lines at first, and the rest a click away, so a busy week does not push the strips off the page.
pub(super) fn trouble(t: &Tokens, a: &Activity, all: bool, more: crate::kit::Click) -> AnyElement {
    if a.trouble.is_empty() {
        return div().into_any_element();
    }
    let shown = if all {
        a.trouble.len()
    } else {
        a.trouble.len().min(6)
    };
    let rest = a.trouble.len() - shown;
    div()
        .px(px(28.))
        .pb(px(14.))
        .child(
            card(t)
                .border_color(mix(t.bad, t.line, 45.))
                .child(heading(t, "TROUBLE", None))
                .child(lines(t, a.trouble.get(..shown).unwrap_or_default()))
                .when(rest > 0, |d| {
                    d.child(
                        div()
                            .id("act-trouble-more")
                            .text_size(px(12.))
                            .text_color(t.cool)
                            .cursor_pointer()
                            .child(text(&format!("and {rest} more · show all")))
                            .on_click(more),
                    )
                }),
        )
        .into_any_element()
}

/// `.tl-list`: the time, the first word in its tone, the rest.
pub(super) fn lines(t: &Tokens, ls: &[Line]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .font_family(MONO_FAMILY)
        .text_size(px(12.))
        .children(ls.iter().map(|l| {
            div()
                .flex()
                .gap(px(10.))
                .whitespace_nowrap()
                .child(
                    div()
                        .w(px(70.))
                        .flex_none()
                        .text_color(t.dim)
                        .child(text(&l.at)),
                )
                .child(
                    div()
                        .w(px(70.))
                        .flex_none()
                        .text_color(tone(t, l.tone))
                        .child(text(l.kind.trim_end_matches(':'))),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(t.text)
                        .child(text(&l.text)),
                )
        }))
}

/// The head's left: Activity, and the stack, the service alone and the span.
fn title(t: &Tokens, path: &str) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(28.))
        .pt(px(22.))
        .pb(px(14.))
        .child(
            div()
                .flex_none()
                .text_size(px(20.))
                .font_weight(w(650.))
                .text_color(t.bright)
                .child("Activity"),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(t.dim)
                .child(text(path)),
        )
        .child(div().flex_1())
}
