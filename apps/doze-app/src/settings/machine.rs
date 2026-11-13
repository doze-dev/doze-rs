//! Settings › Machine (Go `settings.tsx`'s Machine part and `Doctor`): the banner (what is not in place, and Repair
//! behind the system's password prompt), the table by section with each failing part's Copy fix, then what stacks
//! left behind.

use doze_desk::settings::machine::{Part, banner, fixable, sections, to_do};
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::general::part_head;
use super::{SettingsEvent, SettingsView};
use crate::instance::ui::{copy_link, text};
use crate::kit::{Btn, btn};
use crate::theme::{Tokens, mix, ty};
use crate::widgets::{dot, tracked};

pub(super) fn page(
    t: &Tokens,
    v: &SettingsView,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let head = part_head(t, "Machine", "what doze set up here, part by part")
        .child(div().flex_1())
        .child(btn(
            t,
            "copy-report",
            "Copy report",
            "c",
            Btn::Plain,
            Some(Box::new(cx.listener(|v: &mut SettingsView, _, _, cx| {
                v.copy_report(cx)
            }))),
        ));
    let checking = v.parts.is_empty();
    div()
        .flex()
        .flex_col()
        .child(head)
        .child(if checking {
            div()
                .px(px(28.))
                .pb(px(14.))
                .text_color(t.dim)
                .child("asking doze doctor…")
                .into_any_element()
        } else {
            repair_banner(t, v, cx)
        })
        .child(table(t, &v.parts, cx))
        .child(super::left::section(t, v, cx))
        .child(
            div()
                .px(px(28.))
                .pt(px(4.))
                .text_size(px(ty::SM_UP))
                .text_color(t.dim)
                .child("The same report as doze doctor, with the fixes attached."),
        )
}

/// `.banner`: a dot, the line and the line under it; Repair when the app can ask for the password.
fn repair_banner(t: &Tokens, v: &SettingsView, cx: &mut Context<SettingsView>) -> AnyElement {
    let n = to_do(&v.parts);
    let can = v.can_admin() && fixable(&v.parts);
    let (line, sub) = banner(&v.parts, can || v.repairing);
    let tone = if n > 0 { t.warn } else { t.good };
    let mut b = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .mx(px(28.))
        .mb(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(if n > 0 {
            mix(t.warn, t.panel, 50.)
        } else {
            t.line2
        })
        .bg(if n > 0 {
            mix(t.warn, t.panel, 10.)
        } else {
            t.panel
        })
        .child(dot(tone))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(ty::SEMIBOLD)
                        .text_color(t.bright)
                        .child(text(&line)),
                )
                .child(
                    div()
                        .text_size(px(ty::SM))
                        .text_color(t.dim)
                        .child(text(&sub)),
                ),
        );
    if n > 0 && (can || v.repairing) {
        let label = if v.repairing {
            "Repairing…"
        } else {
            "Repair"
        };
        let mut r = btn(t, "repair", label, "", Btn::Primary, None);
        if !v.repairing {
            r = r
                .child(
                    div()
                        .font_weight(ty::REGULAR)
                        .opacity(0.75)
                        .child("(one password)"),
                )
                .on_click(cx.listener(|v: &mut SettingsView, _, _, cx| v.repair(cx)));
        } else {
            r = r.opacity(0.6);
        }
        b = b.child(r);
    }
    b.into_any_element()
}

/// `.chk`: the parts by section, each with its dot, its name, what it says and, when it is not in place, Copy fix.
fn table(t: &Tokens, parts: &[Part], cx: &mut Context<SettingsView>) -> impl IntoElement {
    let mut c = chk(t);
    for (k, (section, title, note)) in sections(parts).into_iter().enumerate() {
        c = c.child(sec_row(t, title, note, k == 0));
        for (i, p) in parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.section == section)
        {
            let fix = p.fix.clone();
            let el = if !p.ok && !p.fix.is_empty() {
                copy_link(t, ("fix", i), "Copy fix")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(fix.clone()));
                        cx.emit(SettingsEvent::Say(
                            "copied the command to the clipboard".into(),
                            false,
                        ));
                    }))
                    .into_any_element()
            } else {
                div().into_any_element()
            };
            c = c.child(row(
                t,
                (if p.ok { t.good } else { t.warn }, false),
                &p.name,
                &p.text,
                el,
            ));
        }
    }
    c
}

/// `.chk`: the bordered table.
pub(super) fn chk(t: &Tokens) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .mx(px(28.))
        .mb(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .overflow_hidden()
}

/// `.cr.sec`: a section's heading in the accent, its note after it.
pub(super) fn sec_row(t: &Tokens, title: &str, note: &str, first: bool) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(28.))
        .px(px(14.))
        .bg(t.well)
        .when(!first, |d| d.border_t_1().border_color(t.line))
        .child(tracked(
            &title.to_uppercase(),
            ty::XS,
            ty::BOLD,
            t.accent,
            0.88,
        ))
        .child(div().flex_1())
        .child(
            div()
                .text_size(px(ty::XS))
                .text_color(t.dim)
                .child(text(note)),
        )
}

/// `.cr`: the dot, the name (170 wide), what it says, and what to do.
/// Left behind's rows share the width between what and where (Go `.leftovers .cr`): `wide`.
pub(super) fn row(
    t: &Tokens,
    (tone, wide): (gpui::Hsla, bool),
    name: &str,
    said: &str,
    act: AnyElement,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(38.))
        .px(px(14.))
        .py(px(4.))
        .border_t_1()
        .border_color(t.line)
        .child(div().w(px(22.)).flex_none().child(dot(tone)))
        .child(
            div()
                .when(wide, |d| d.flex_1().min_w_0())
                .when(!wide, |d| d.w(px(170.)).flex_none())
                .font_weight(ty::SEMIBOLD)
                .text_color(t.bright)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(text(name)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(wide, |d| d.font_family(crate::widgets::MONO_FAMILY))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(text(said)),
        )
        .child(div().flex_none().child(act))
}
