//! A task's page, as the TaskNotRun and TaskRan boards draw it: before it ever ran, what Run will do; after, its
//! output (the run picked, the newest until another is) beside every run kept, with how long each took as a bar.

use doze_desk::page::instance::InstancePage;
use doze_desk::page::parts::RunRow;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px, relative};

use super::InstanceView;
use super::ui::{self, card, heading, step_num, text};
use crate::theme::Tokens;
use crate::widgets::{MONO_FAMILY, w};

pub(crate) fn task(t: &Tokens, v: &mut InstanceView, cx: &mut Context<InstanceView>) -> AnyElement {
    if v.page.runs.is_empty() {
        not_run(t, v)
    } else {
        last_run(t, v, cx)
    }
}

fn not_run(t: &Tokens, v: &InstanceView) -> AnyElement {
    let list = step_list(t, run_steps(&v.page));
    let left = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .flex_1()
        .min_w_0()
        .child(div().pt(px(14.)).text_size(px(18.)).font_weight(w(650.)).text_color(t.bright).child(text(&if v.page.look == Some(doze_display::Look::Running) { format!("{} is running now.", v.name) } else { format!("{} has not run yet.", v.name) })))
        .child(div().text_color(t.dim).child("A task runs once when you say so, and then gets out of the way: it keeps nothing awake and nothing waits on it. Run does the steps below, and the output lands on this page as it comes."))
        .child(list)
        .child(
            div()
                .flex()
                .flex_wrap()
                .text_size(px(12.5))
                .text_color(t.dim)
                .items_baseline()
                .child("From a terminal: ")
                .child(div().font_family(MONO_FAMILY).text_color(t.bright).child(text(&format!("doze task {}", v.name))))
                .child(". From a script: the same, with ")
                .child(div().font_family(MONO_FAMILY).child("--json"))
                .child("."),
        );
    let right = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .flex_1()
        .min_w_0()
        .children(command(t, v))
        .child(
            card(t).child(heading(t, "Output", None)).child(
                ui::code(t)
                    .min_h(px(120.))
                    .text_color(t.dim)
                    .child("nothing yet · appears here while it runs, and stays after"),
            ),
        );
    two(left, right, 1.2)
}

/// What Run will do, step by step: wake what it needs, run the command, report.
fn run_steps(p: &InstancePage) -> Vec<(String, String)> {
    let needs: Vec<&str> = p.needs.iter().map(|n| n.name.as_str()).collect();
    let mut steps: Vec<(String, String)> = Vec::new();
    if !needs.is_empty() {
        let awake = p
            .needs
            .iter()
            .all(|n| n.look == Some(doze_display::Look::Awake));
        steps.push((
            format!("wake {}", needs.join(", ")),
            if awake {
                "awake now, so nothing to wait for"
            } else {
                "asleep, about a second"
            }
            .to_owned(),
        ));
    }
    let run = if p.run.is_empty() {
        "the command"
    } else {
        &p.run
    };
    steps.push((
        format!("run {run}"),
        "with the stack's variables".to_owned(),
    ));
    steps.push((
        "report".to_owned(),
        "exit code, how long it took, the output kept here; a notification only if it fails"
            .to_owned(),
    ));
    steps
}

/// The steps as a numbered list, each with what it does and how.
fn step_list(t: &Tokens, steps: Vec<(String, String)>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .children(steps.into_iter().enumerate().map(|(i, (what, how))| {
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .px(px(14.))
                .py(px(12.))
                .when(i > 0, |d| d.border_t_1().border_color(t.line))
                .child(step_num(t, i + 1))
                .child(
                    div()
                        .w(px(180.))
                        .flex_none()
                        .text_color(t.bright)
                        .font_weight(w(600.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(text(&what)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(t.dim)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(text(&how)),
                )
        }))
}

fn two(left: gpui::Div, right: gpui::Div, grow: f32) -> AnyElement {
    div()
        .id("task")
        .flex()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .gap(px(14.))
        .px(px(28.))
        .py(px(16.))
        .child(left.flex_grow(grow).flex_basis(relative(0.)))
        .child(right.flex_grow(1.).flex_basis(relative(0.)))
        .into_any_element()
}

/// The command card: what it runs and what it needs; nothing when it says neither.
fn command(t: &Tokens, v: &InstanceView) -> Option<AnyElement> {
    let p = &v.page;
    if p.run.is_empty() && p.needs.is_empty() {
        return None;
    }
    let mut c = card(t).child(heading(t, "Command", None));
    if !p.run.is_empty() {
        c = c.child(row(
            t,
            "run",
            div()
                .font_family(MONO_FAMILY)
                .text_size(px(12.))
                .text_color(t.bright)
                .child(text(&p.run)),
        ));
    }
    if !p.needs.is_empty() {
        let names: Vec<&str> = p.needs.iter().map(|n| n.name.as_str()).collect();
        c = c.child(
            row(t, "needs", div().child(text(&names.join(", ")))).child(
                div()
                    .text_size(px(12.))
                    .text_color(t.dim)
                    .child("woken first if asleep"),
            ),
        );
    }
    Some(c.into_any_element())
}

fn row(t: &Tokens, k: &str, v: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(26.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_color(t.dim)
                .child(text(k)),
        )
        .child(
            v.flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap(),
        )
}

fn last_run(t: &Tokens, v: &mut InstanceView, cx: &mut Context<InstanceView>) -> AnyElement {
    let runs = v.page.runs.clone();
    let shown = runs
        .iter()
        .find(|r| Some(r.seq) == v.run_shown)
        .or(runs.first())
        .cloned();
    let sub = shown.as_ref().map(|r| {
        let end = if r.done {
            format!("exit {}", r.exit)
        } else {
            "running".to_owned()
        };
        format!("· run {} · {} · {end} · {}", r.seq, r.when, r.took)
    });
    let copied = v.is_copied("output");
    let output = card(t)
        .flex_1()
        .min_h(px(320.))
        .h_full()
        .child(
            heading(t, "Output", sub.as_deref())
                .child(div().flex_1())
                .child(
                    ui::copy_link(t, "copy-output", if copied { "Copied" } else { "Copy" })
                        .on_click(cx.listener(|v, _, _, cx| v.copy_output(cx))),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(260.))
                .rounded(px(10.))
                .border_1()
                .border_color(t.line)
                .bg(t.well)
                .overflow_hidden()
                .children(v.output().map(|l| l.into_any_element())),
        );
    let longest = runs.iter().map(|r| r.secs).fold(0.001, f64::max);
    let table = div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(t.line)
        .bg(t.panel)
        .overflow_hidden()
        .child(runs_head(t))
        .children(runs.iter().map(|r| {
            run_row(
                t,
                r,
                shown.as_ref().map(|s| s.seq) == Some(r.seq),
                longest,
                cx,
            )
        }));
    let right = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .flex_1()
        .min_w_0()
        .child(card(t).child(heading(t, "Runs", Some("· kept for this stack, newest first"))).child(table))
        .children(command(t, v))
        .child(div().text_size(px(12.5)).text_color(t.dim).child("A task keeps nothing awake once it ends. A failed run shows a notification and the row goes red until the next run."));
    let left = div().flex().flex_col().flex_1().min_w_0().child(output);
    two(left, right, 1.3)
}

fn cols(t: &Tokens) -> [gpui::Div; 4] {
    let _ = t;
    [
        div().w(px(68.)).flex_none(),
        div().w(px(96.)).flex_none(),
        div().w(px(34.)).flex_none(),
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap(),
    ]
}

fn runs_head(t: &Tokens) -> AnyElement {
    let [a, b, c, d] = cols(t);
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(28.))
        .px(px(12.))
        .bg(t.well)
        .text_size(px(11.))
        .font_weight(w(600.))
        .text_color(t.dim)
        .child(a.child("WHEN"))
        .child(b.child("TOOK"))
        .child(c.child("EXIT"))
        .child(d.child("STARTED BY"))
        .child(div().w(px(48.)))
        .into_any_element()
}

fn run_row(
    t: &Tokens,
    r: &RunRow,
    on: bool,
    longest: f64,
    cx: &mut Context<InstanceView>,
) -> AnyElement {
    let [a, b, c, d] = cols(t);
    let failed = r.done && !r.ok;
    let bar = ((r.secs / longest) * 40.0).round().max(6.0) as f32;
    let seq = r.seq;
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(34.))
        .px(px(12.))
        .border_t_1()
        .border_color(t.line)
        .when(on, |d| d.bg(t.pick))
        .child(
            a.font_family(MONO_FAMILY)
                .text_size(px(12.))
                .child(text(&r.when)),
        )
        .child(
            b.flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .child(div().w(px(bar)).h(px(6.)).rounded(px(3.)).bg(if failed {
                    t.bad
                } else {
                    t.cool
                }))
                .child(text(&r.took)),
        )
        .child(
            c.text_color(if !r.done {
                t.cool
            } else if r.ok {
                t.good
            } else {
                t.bad
            })
            .child(text(&if r.done {
                r.exit.to_string()
            } else {
                "…".to_owned()
            })),
        )
        .child(d.text_size(px(12.)).text_color(t.dim).child(text(&r.why)))
        .child(
            ui::copy_link(t, ("run-out", seq), if on { "Shown" } else { "Output" })
                .w(px(48.))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.run_shown = Some(seq);
                    v.sync_run(cx);
                    cx.notify();
                })),
        )
        .into_any_element()
}

impl InstanceView {
    /// Copies the shown run's output (the newest's when none was picked), as kept.
    pub(crate) fn copy_output(&mut self, cx: &mut Context<Self>) {
        let run = self
            .page
            .runs
            .iter()
            .find(|r| Some(r.seq) == self.run_shown)
            .or(self.page.runs.first())
            .map_or(0, |r| r.log);
        let name = self.name.clone();
        let text = self
            .hub
            .with_logs(&self.key, |ring| {
                let run = if run > 0 {
                    run
                } else {
                    doze_desk::page::logs::runs_of(ring, &name)
                        .last()
                        .copied()
                        .unwrap_or(0)
                };
                ring.of(&name)
                    .filter(|l| l.run == run)
                    .map(|l| l.line.to_owned())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        self.copy("output", &text, cx);
    }
}
