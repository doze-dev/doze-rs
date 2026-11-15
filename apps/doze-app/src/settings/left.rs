//! Left behind (Go `settings.tsx`'s `Leftovers`): what stacks left out of step with doze, by kind, six of a kind at
//! first. Each has its own button; Clean up N does all but the data, which is deleted one folder at a time after an
//! ask in its own row (Delete it / Keep). Look again reads the machine afresh.

use doze_desk::settings::Leftover;
use doze_desk::settings::leftovers::{SHOWN, counts, kinds, summary};
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::SettingsView;
use super::machine::{chk, row, sec_row};
use crate::instance::ui::{copy_link, text};
use crate::kit::{Btn, btn};
use crate::theme::{Tokens, ty};

pub(super) fn section(t: &Tokens, v: &SettingsView, cx: &mut Context<SettingsView>) -> AnyElement {
    let Some(items) = &v.left else {
        return div()
            .px(px(28.))
            .pb(px(14.))
            .text_color(t.dim)
            .child(if v.looking {
                "looking for what stacks left behind…"
            } else {
                ""
            })
            .into_any_element();
    };
    let (safe, _) = counts(items);
    let busy = !v.busy.is_empty();
    let mut head = title(t, &summary(items)).child(
        btn(
            t,
            "look-again",
            if v.looking {
                "Looking…"
            } else {
                "Look again"
            },
            "",
            Btn::Quiet,
            None,
        )
        .when(!busy && !v.looking, |b| {
            b.on_click(cx.listener(|v: &mut SettingsView, _, _, cx| v.look(cx)))
        }),
    );
    if safe > 0 {
        let label = if v.busy == "all" {
            "Cleaning up…".to_owned()
        } else {
            format!("Clean up {safe}")
        };
        head = head.child(
            btn(t, "clean-up", &label, "", Btn::Primary, None)
                .when(busy, |b| b.opacity(0.6))
                .when(!busy, |b| b.on_click(cx.listener(|v: &mut SettingsView, _, _, cx| v.clear("all", cx))))
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new(
                        "Stops the stray processes and removes the stale routes; data is never deleted this way",
                    )
                    .build(window, cx)
                }),
        );
    }
    let mut out = div().flex().flex_col().child(head);
    for k in kinds(items) {
        out = out.child(kind_table(t, v, items, k, cx));
    }
    out.into_any_element()
}

fn kind_table(
    t: &Tokens,
    v: &SettingsView,
    items: &[Leftover],
    kind: &'static str,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let of: Vec<&Leftover> = items.iter().filter(|l| l.kind == kind).collect();
    let whole = v.whole.contains(&kind);
    let mut c = chk(t).child(sec_row(
        t,
        Leftover::heading(kind),
        &of.len().to_string(),
        true,
    ));
    for l in of.iter().take(if whole { usize::MAX } else { SHOWN }) {
        c = c.child(row(
            t,
            (if l.danger { t.bad } else { t.warn }, true),
            &l.what,
            &l.detail,
            act(t, v, l, cx),
        ));
    }
    if !whole && of.len() > SHOWN {
        c = c.child(
            div()
                .id(gpui::SharedString::from(format!("more-{kind}")))
                .px(px(14.))
                .py(px(8.))
                .border_t_1()
                .border_color(t.line)
                .text_size(px(ty::XS))
                .text_color(t.accent)
                .cursor_pointer()
                .child(text(&format!(
                    "and {} more like these · show all",
                    of.len() - SHOWN
                )))
                .on_click(cx.listener(move |v: &mut SettingsView, _, _, cx| {
                    v.whole.push(kind);
                    cx.notify();
                })),
        );
    }
    c
}

/// A row's button: its action, or the ask before data is deleted, or "…" while it (or Clean up) runs.
fn act(t: &Tokens, v: &SettingsView, l: &Leftover, cx: &mut Context<SettingsView>) -> AnyElement {
    let id = l.id.clone();
    if v.asking == l.id {
        let yes = id.clone();
        return div()
            .flex()
            .gap(px(6.))
            .child(
                btn(t, "delete-it", "Delete it", "", Btn::Plain, None)
                    .bg(t.bad)
                    .text_color(t.ink)
                    .border_color(t.bad)
                    .on_click(cx.listener(move |v: &mut SettingsView, _, _, cx| v.clear(&yes, cx))),
            )
            .child(
                btn(t, "keep-it", "Keep", "", Btn::Quiet, None).on_click(cx.listener(
                    |v: &mut SettingsView, _, _, cx| {
                        v.asking.clear();
                        cx.notify();
                    },
                )),
            )
            .into_any_element();
    }
    let running = v.busy == l.id || v.busy == "all";
    let label = if running { "…" } else { l.action.as_str() };
    let mut b = copy_link(t, gpui::SharedString::from(format!("left-{}", l.id)), label);
    if v.busy.is_empty() {
        let danger = l.danger;
        b = b.on_click(cx.listener(move |v: &mut SettingsView, _, _, cx| {
            if danger {
                v.asking = id.clone();
                cx.notify();
            } else {
                v.clear(&id, cx);
            }
        }));
    } else {
        b = b.opacity(0.5);
    }
    b.into_any_element()
}

/// `.lh`: Left behind, and how many things.
fn title(t: &Tokens, said: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .mx(px(28.))
        .mt(px(6.))
        .mb(px(8.))
        .child(
            div()
                .flex_none()
                .text_size(px(ty::SM))
                .font_weight(ty::SEMIBOLD)
                .text_color(t.bright)
                .child("Left behind"),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(ty::XS))
                .text_color(t.dim)
                .child(text(said)),
        )
        .child(div().flex_1())
}
