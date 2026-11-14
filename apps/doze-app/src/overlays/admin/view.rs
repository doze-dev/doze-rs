//! The run sheet drawn: the steps as they will run (and the root script each prints), the output as it comes, how it
//! ended, and the one button that runs it.

use doze_desk::settings::{Planned, Said};
use gpui::{AnyElement, Context, IntoElement, Render, SharedString, Window, div, prelude::*, px};

use super::{AdminView, Phase, Shown};
use crate::actions::Confirm;
use crate::kit::{Btn, btn};
use crate::theme::{Tokens, mix, tokens, ty};
use crate::widgets::MONO_FAMILY;

/// A block of monospaced text, its own scroll past `max`.
fn mono(t: &Tokens, id: impl Into<gpui::ElementId>, text: &str, max: f32) -> AnyElement {
    div()
        .id(id)
        .max_h(px(max))
        .overflow_y_scroll()
        .px(px(10.))
        .py(px(7.))
        .rounded(px(7.))
        .bg(t.well)
        .border_1()
        .border_color(t.line)
        .font_family(MONO_FAMILY)
        .text_size(px(ty::XS_UP))
        .line_height(px(ty::LH_16))
        .text_color(t.text)
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

fn tag(t: &Tokens, asks: bool) -> impl IntoElement {
    let (words, c) = if asks {
        ("one password", t.warn)
    } else {
        ("no password", t.dim)
    };
    div()
        .flex_none()
        .px(px(7.))
        .rounded(px(9.))
        .border_1()
        .border_color(mix(c, t.panel, 45.))
        .text_size(px(ty::XS))
        .text_color(c)
        .child(words)
}

/// Small dim words under a step.
fn dim(t: &Tokens, text: String) -> gpui::Div {
    div()
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .child(SharedString::from(text))
}

/// A doze command's step: the command, where it runs as the person, and the root script it printed.
fn doze_body(
    t: &Tokens,
    i: usize,
    (s, dir): (&Shown, Option<&std::path::Path>),
    v: &AdminView,
    mut body: gpui::Div,
) -> gpui::Div {
    let line = v
        .cli
        .as_ref()
        .map(|c| s.step.command(c))
        .unwrap_or_default();
    body = body.child(mono(t, ("cmd", i), &format!("$ {line}"), 60.));
    let at = dir.map_or_else(|| v.hub.home().to_path_buf(), std::path::Path::to_path_buf);
    body = body.child(dim(
        t,
        format!(
            "as you, in {}",
            doze_display::tilde(&at.to_string_lossy(), std::env::var("HOME").ok().as_deref())
        ),
    ));
    if !s.asks {
        return body;
    }
    match &s.root {
        Some(r) if !r.is_empty() => body
            .child(dim(
                t,
                "then, as root after the password prompt, it runs:".into(),
            ))
            .child(mono(t, ("root", i), r, 150.)),
        _ if v.phase == Phase::Planning => {
            body.child(dim(t, "asking doze what it runs as root…".into()))
        }
        _ => body.child(dim(
            t,
            "it asks for the password only for what is not set up yet".into(),
        )),
    }
}

/// A step: its number, what it is for, whether it asks, the command and where, and the root script it prints.
fn step(t: &Tokens, i: usize, s: &Shown, v: &AdminView) -> AnyElement {
    let cli_line = v
        .cli
        .as_ref()
        .map(|c| s.step.command(c))
        .unwrap_or_default();
    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .font_weight(ty::SEMIBOLD)
                        .text_color(t.bright)
                        .child(SharedString::from(s.step.what().to_owned())),
                )
                .child(tag(t, s.asks)),
        );
    match &s.step {
        Planned::Doze { dir, .. } => body = doze_body(t, i, (s, dir.as_deref()), v, body),
        Planned::Admin { .. } => {
            body = body
                .child(dim(t, "as root, after the password prompt:".into()))
                .child(mono(t, ("root", i), &cli_line, 150.));
        }
        Planned::Stop { .. } | Planned::Delete { .. } => {
            body = body.child(mono(t, ("cmd", i), &format!("$ {cli_line}"), 60.));
        }
    }
    div()
        .flex()
        .gap(px(10.))
        .py(px(10.))
        .border_t_1()
        .border_color(t.line)
        .child(
            div()
                .w(px(18.))
                .flex_none()
                .text_color(t.dim)
                .child(SharedString::from(format!("{}.", i + 1))),
        )
        .child(crate::outward::Probe::probe(
            body,
            format!("admin-step-{i}"),
            format!("{} · {cli_line}", s.step.what()),
        ))
        .into_any_element()
}

/// What ran, as it printed it.
fn output(t: &Tokens, v: &AdminView) -> AnyElement {
    let lines = v.lines.iter().enumerate().map(|(i, l)| {
        let (text, c) = match l {
            Said::Run(s) => (format!("$ {s}"), t.bright),
            Said::Line(s) => (s.clone(), t.text),
            Said::Note(s) => (format!("· {s}"), t.dim),
        };
        div()
            .id(("out", i))
            .text_color(c)
            .child(SharedString::from(text))
    });
    div()
        .id("admin-out")
        .track_scroll(&v.scroll)
        .h(px(220.))
        .overflow_y_scroll()
        .px(px(10.))
        .py(px(7.))
        .rounded(px(7.))
        .bg(t.well)
        .border_1()
        .border_color(t.line)
        .font_family(MONO_FAMILY)
        .text_size(px(ty::XS_UP))
        .line_height(px(ty::LH_16))
        .children(lines)
        .into_any_element()
}

impl AdminView {
    fn status(&self, t: &Tokens) -> AnyElement {
        let (words, c) = match &self.phase {
            Phase::Planning => ("asking doze what it will run…".to_owned(), t.dim),
            Phase::Ready => (
                "Nothing has run yet. The system asks for the password itself; doze never sees it."
                    .to_owned(),
                t.dim,
            ),
            Phase::Running => {
                let now = self
                    .lines
                    .iter()
                    .rev()
                    .find_map(|l| match l {
                        Said::Run(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                (format!("running {now}…"), t.cool)
            }
            Phase::Checking => (
                "every step ran; doctor is looking again…".to_owned(),
                t.cool,
            ),
            Phase::Done(said, bad) => (said.clone(), if *bad { t.bad } else { t.good }),
        };
        crate::outward::Probe::probe(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(ty::SM))
                .text_color(c)
                .child(SharedString::from(words.clone())),
            "admin-status",
            words,
        )
        .into_any_element()
    }

    fn foot(&self, t: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let close = Box::new(cx.listener(|v: &mut AdminView, _, _, cx| v.close(cx)));
        let mut f = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pt(px(4.))
            .child(self.status(t));
        match self.phase {
            Phase::Ready => {
                let label = match self.prompts() {
                    0 => "Run".to_owned(),
                    1 => "Run (one password)".to_owned(),
                    n => format!("Run ({n} passwords)"),
                };
                let run = Box::new(cx.listener(|v: &mut AdminView, _, _, cx| v.run(cx)));
                f = f
                    .child(btn(
                        t,
                        "admin-cancel",
                        "Cancel",
                        "esc",
                        Btn::Plain,
                        Some(close),
                    ))
                    .child(btn(t, "admin-run", &label, "⏎", Btn::Primary, Some(run)));
            }
            Phase::Planning | Phase::Running | Phase::Checking => {
                f = f.child(btn(t, "admin-hide", "Hide", "esc", Btn::Quiet, Some(close)));
            }
            Phase::Done(..) => {
                f = f.child(btn(
                    t,
                    "admin-close",
                    "Close",
                    "esc",
                    Btn::Plain,
                    Some(close),
                ));
            }
        }
        f
    }
}

impl Render for AdminView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let steps: Vec<AnyElement> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| step(&t, i, s, self))
            .collect();
        let ran = !self.lines.is_empty();
        crate::overlays::scrim::panel(&t, 720., 10., self.job.title())
            .id("admin")
            .key_context("Sheet")
            .track_focus(&self.focus)
            .on_action(cx.listener(|v, _: &Confirm, _, cx| v.run(cx)))
            .child(
                div()
                    .id("admin-steps")
                    .max_h(px(if ran { 200. } else { 440. }))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(steps),
            )
            .when(ran, |d| d.child(output(&t, self)))
            .child(self.foot(&t, cx))
    }
}
