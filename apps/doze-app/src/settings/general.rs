//! Settings › General (Go `settings.tsx`, the General part): the mode, the rows, the colour theme (each of the five
//! drawn in the mode on screen: its ground, its accent, good and cool), Startup, and where it is all saved.

use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px};

use super::{SettingsEvent, SettingsView};
use crate::instance::ui::text;
use crate::theme::{Choice, Density, Mode, Palette, Tokens, swatch, tokens};
use crate::widgets::{icon, seg, switch, w};

/// The head of a part: its name, and a line after it.
pub(super) fn part_head(t: &Tokens, title: &str, path: &str) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(28.))
        .pt(px(22.))
        .pb(px(18.))
        .child(
            div()
                .text_size(px(20.))
                .font_weight(w(650.))
                .text_color(t.bright)
                .child(text(title)),
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
}

/// `.set`: the key with its small line at the left (200 wide), what sets it at the right.
pub(super) fn set_row(t: &Tokens, k: &str, small: &str, v: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .gap(px(24.))
        .px(px(28.))
        .pb(px(14.))
        .max_w(px(900.))
        .child(
            div()
                .w(px(200.))
                .flex_none()
                .pt(px(4.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .font_weight(w(600.))
                        .text_color(t.bright)
                        .child(text(k)),
                )
                .when(!small.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(t.dim)
                            .child(text(small)),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .flex_1()
                .min_w_0()
                .child(v),
        )
}

pub(super) fn page(
    t: &Tokens,
    v: &SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let choice = *cx.global::<Choice>();
    let dark = tokens(cx).dark;
    // Go's words name macOS; elsewhere, the system.
    let os = if std::env::consts::OS == "macos" {
        "macOS"
    } else {
        "the system"
    };
    let mode_small = if choice.mode == Mode::System {
        format!(
            "following {os}: {} now",
            if dark { "dark" } else { "light" }
        )
    } else {
        format!("the same whatever {os} does")
    };
    let _ = window;
    div()
        .flex()
        .flex_col()
        .child(part_head(
            t,
            "General",
            &format!(
                "saved as you change them · {}",
                crate::prefs::page::shown_path()
            ),
        ))
        .child(set_row(t, "Mode", &mode_small, modes(t, choice, cx)))
        .child(set_row(
            t,
            "List rows",
            "how much room each row gets, on Home and in a stack's List",
            rows(t, v.density, cx),
        ))
        .child(set_row(
            t,
            "Colour theme",
            "the terminal's five, each in light and dark",
            swatches(t, choice, dark, cx),
        ))
        .child(set_row(
            t,
            "Startup",
            "the bat is light; the window is not kept",
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(toggle(
                    t,
                    "login",
                    v.login,
                    "Open at login, in the menu bar only",
                    cx,
                    |v, cx| v.set_login(cx),
                ))
                .child(toggle(
                    t,
                    "reopen",
                    v.reopen,
                    "Reopen the page I had, when the window opens",
                    cx,
                    |v, cx| {
                        v.reopen = !v.reopen;
                        crate::prefs::page::save_reopen(v.reopen);
                        cx.notify();
                    },
                )),
        ))
}

fn modes(t: &Tokens, choice: Choice, cx: &mut Context<SettingsView>) -> impl IntoElement {
    let items = [
        (Mode::System, "System"),
        (Mode::Light, "Light"),
        (Mode::Dark, "Dark"),
    ]
    .into_iter()
    .map(|(m, word)| {
        let click: crate::widgets::OnClick = Box::new(cx.listener(move |_, _, _, cx| {
            cx.emit(SettingsEvent::Choose(Choice { mode: m, ..choice }));
        }));
        (
            div().child(word).into_any_element(),
            choice.mode == m,
            Some(click),
        )
    })
    .collect();
    div().flex().child(seg(t, "set-mode", 12., items))
}

fn rows(t: &Tokens, on: Density, cx: &mut Context<SettingsView>) -> impl IntoElement {
    let items = Density::ALL
        .into_iter()
        .map(|d| {
            let path = match d {
                Density::Compact => "icons/lines-3.svg",
                Density::Cosy => "icons/lines-4.5.svg",
                Density::Roomy => "icons/lines-6.svg",
            };
            let lit = d == on;
            let click: crate::widgets::OnClick =
                Box::new(cx.listener(move |v: &mut SettingsView, _, _, cx| {
                    v.density = d;
                    cx.emit(SettingsEvent::Density(d));
                    cx.notify();
                }));
            let label = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon(path, 14., if lit { t.bright } else { t.dim }))
                .child(capital(d.word()));
            (label.into_any_element(), lit, Some(click))
        })
        .collect();
    div().flex().child(seg(t, "set-rows", 11., items))
}

fn capital(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// `.swatches`: each palette as a little window in the mode on screen, the chosen one ringed in the accent.
fn swatches(
    t: &Tokens,
    choice: Choice,
    dark: bool,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(10.))
        .children(Palette::ALL.into_iter().map(|p| {
            let on = choice.palette == p;
            let look = swatch(p, dark).unwrap_or(*t);
            let bar = |c: gpui::Hsla, pct: f32| {
                div().h(px(6.)).rounded(px(3.)).w(gpui::relative(pct)).bg(c)
            };
            div()
                .id(p.name())
                .w(px(110.))
                .rounded(px(9.))
                .border_1()
                .border_color(if on { t.accent } else { t.line2 })
                .when(on, |d| d.border_2())
                .bg(t.panel)
                .overflow_hidden()
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(SettingsEvent::Choose(Choice {
                        palette: p,
                        ..choice
                    }));
                }))
                .child(
                    div()
                        .h(px(54.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .p(px(8.))
                        .bg(look.bg)
                        .child(bar(look.accent, 0.7))
                        .child(bar(look.good, 0.4))
                        .child(bar(look.cool, 0.55)),
                )
                .child(
                    div()
                        .px(px(8.))
                        .py(px(5.))
                        .text_size(px(12.))
                        .text_color(t.text)
                        .child(capital(p.name())),
                )
        }))
}

/// `.radio` with a switch: the switch, then what it does.
fn toggle(
    t: &Tokens,
    id: &'static str,
    on: bool,
    words: &str,
    cx: &mut Context<SettingsView>,
    flip: fn(&mut SettingsView, &mut Context<SettingsView>),
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .text_color(t.text)
        .on_click(cx.listener(move |v, _, _, cx| flip(v, cx)))
        .child(switch(t, on, false, false))
        .child(text(words))
        .into_any_element()
}
