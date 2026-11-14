//! Settings › About (Go `settings.tsx`'s About part and `UpdateLine`): the version, whether a newer doze is out
//! (asked when About opens or on Check now, never in the background), What's new, the command that updates, and
//! doze-dev.com.

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::general::{part_head, set_row};
use super::{SettingsEvent, SettingsView};
use crate::instance::ui::text;
use crate::kit::{Btn, btn};
use crate::theme::{Tokens, ty};
use crate::widgets::Mono as _;
use doze_desk::settings::Update;

pub(super) fn page(
    t: &Tokens,
    v: &SettingsView,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(part_head(t, "About", &format!("doze {}", crate::VERSION)))
        .child(set_row(
            t,
            "doze",
            "this app, and the doze command it runs for stacks",
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().mono_face().text_color(t.bright).child(crate::VERSION))
                .child(update_line(t, v, cx))
                .child(div().flex().child(btn(
                    t,
                    "site",
                    "doze-dev.com",
                    "",
                    Btn::Plain,
                    Some(Box::new(|_, _, cx| cx.open_url("https://doze-dev.com"))),
                ))),
        ))
}

/// What GitHub said: a newer doze (what's new, the command), the newest already, a development build, or why it could
/// not tell; then Check now.
fn update_line(t: &Tokens, v: &SettingsView, cx: &mut Context<SettingsView>) -> AnyElement {
    let label = if v.checking {
        "Checking…"
    } else {
        "Check now"
    };
    let check = btn(t, "check-now", label, "", Btn::Quiet, None).when(!v.checking, |b| {
        b.on_click(cx.listener(|v: &mut SettingsView, _, _, cx| v.check(true, cx)))
    });
    let Some(u) = &v.update else {
        return div()
            .text_color(t.dim)
            .child("checking for a newer doze…")
            .into_any_element();
    };
    let col = div().flex().flex_col().gap(px(6.));
    let col = if u.newer {
        newer(t, u, col, cx)
    } else {
        let said = said(u);
        col.when(!said.is_empty(), |d| {
            d.child(div().text_color(t.dim).child(text(&said)))
        })
    };
    col.child(div().flex().child(check)).into_any_element()
}

/// The words when no newer doze is out (Go `UpdateLine`).
fn said(u: &Update) -> String {
    if !u.error.is_empty() {
        u.error.clone()
    } else if u.dev {
        match u.latest.as_str() {
            "" => "A development build.".into(),
            l => format!("A development build; the newest release is {l}."),
        }
    } else if !u.latest.is_empty() {
        match u.checked.as_str() {
            "" => "This is the newest doze.".into(),
            c => format!("This is the newest doze (checked {c})."),
        }
    } else {
        String::new()
    }
}

/// A newer doze: which and when, how to update, What's new and the command.
fn newer(t: &Tokens, u: &Update, col: gpui::Div, cx: &mut Context<SettingsView>) -> gpui::Div {
    let url = u.url.clone();
    let when = if u.published.is_empty() {
        String::new()
    } else {
        format!(" ({})", u.published)
    };
    let bright = gpui::HighlightStyle {
        color: Some(t.bright),
        font_weight: Some(ty::SEMIBOLD),
        ..Default::default()
    };
    let copy = cx.listener(|_, _, _, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("brew upgrade doze".into()));
        cx.emit(SettingsEvent::Say(
            "copied the command to the clipboard".into(),
            false,
        ));
    });
    col.child(
        div()
            .font_weight(ty::SEMIBOLD)
            .text_color(t.good)
            .child(text(&format!("doze {} is out{when}", u.latest))),
    )
    .child(
        div()
            .text_color(t.dim)
            .child(gpui::StyledText::new(UPGRADE).with_highlights([(11..28, bright)])),
    )
    .child(
        div()
            .flex()
            .gap(px(8.))
            .child(btn(
                t,
                "whats-new",
                "What's new",
                "",
                Btn::Primary,
                Some(Box::new(move |_, _, cx| cx.open_url(&url))),
            ))
            .child(btn(
                t,
                "copy-upgrade",
                "Copy the command",
                "",
                Btn::Plain,
                Some(Box::new(copy)),
            )),
    )
}

/// What updating takes; the command (11..28) is drawn bright.
const UPGRADE: &str = "To update: brew upgrade doze, or download it from the release page. Running stacks keep running.";
