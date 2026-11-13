//! The stack page's verbs (Go `overview.tsx`: `Switch`, `toggle`, the head's switch, the keys): a row's switch wakes,
//! sleeps (asking when in use) or turns a service back on; a task's button runs it; a stopped stack's row starts the
//! stack with that one; the head's switch is Up as last time or Down. `w`, `s`, `r`, `p` act on the selected row, `u`
//! and `c` start a stopped stack. Busy and failed show as banners over the strip ([`banners`]).

use doze_control::Verb;
use doze_desk::act::{Busy, Failure, StackAction};
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::{OverviewView, all_rows};
use crate::kit::{Btn, btn, s};
use crate::theme::{Tokens, mix};
use crate::widgets::{MONO_FAMILY, w};

impl OverviewView {
    fn acts(
        &self,
        cx: &gpui::App,
    ) -> Option<(gpui::Entity<crate::act::Acts>, doze_desk::StackKey)> {
        Some((crate::act::acts(cx)?, self.key.clone()?))
    }

    /// What the stack shown is busy doing, and its last failed action.
    pub(super) fn busy(&self, cx: &gpui::App) -> (Option<Busy>, Option<Failure>) {
        let Some((a, k)) = self.acts(cx) else {
            return (None, None);
        };
        let a = a.read(cx);
        (a.busy_of(&k), a.failed.get(&k).cloned())
    }

    /// A whole-stack action on the stack shown.
    pub(crate) fn stack_act(&mut self, action: StackAction, cx: &mut Context<Self>) {
        if let Some((a, k)) = self.acts(cx) {
            a.update(cx, |a, cx| a.stack(&k, action, cx));
        }
    }

    /// The head's switch: Down when up, else Up as last time.
    pub(super) fn flip_stack(&mut self, cx: &mut Context<Self>) {
        if let Some(problem) = self
            .home
            .stopped
            .as_ref()
            .map(|s| s.problem.clone())
            .filter(|p| !p.is_empty())
        {
            self.say(&format!("{}: {problem}", self.home.stack), cx);
            return;
        }
        let action = if self.home.running {
            StackAction::Down
        } else {
            StackAction::UpLast
        };
        self.stack_act(action, cx);
    }

    /// The start sheet (`c`, or Choose what starts…), for a stopped stack.
    pub(crate) fn choose_start(&mut self, cx: &mut Context<Self>) {
        if self.home.running {
            return;
        }
        if let Some((a, k)) = self.acts(cx) {
            a.update(cx, |a, cx| a.choose_start(&k, cx));
        }
    }

    /// `u`: up as last time, for a stopped stack.
    pub(crate) fn up_last(&mut self, cx: &mut Context<Self>) {
        if !self.home.running {
            self.flip_stack(cx);
        }
    }

    fn verb(&mut self, name: &str, verb: Verb, cx: &mut Context<Self>) {
        if let Some((a, k)) = self.acts(cx) {
            let name = name.to_owned();
            a.update(cx, |a, cx| a.verb(&k, &name, verb, cx));
        }
    }

    /// A row's switch, or its Run (Go `Switch`, `toggle`).
    pub(super) fn toggle_row(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(r) = all_rows(&self.home)
            .into_iter()
            .find(|r| r.name == name)
            .cloned()
        else {
            return;
        };
        if r.down {
            if r.kind != "task" && !r.off {
                self.stack_act(StackAction::Up(vec![r.name.clone()]), cx);
            }
            return;
        }
        if r.kind == "task" {
            return self.verb(name, Verb::Task, cx);
        }
        if r.paused || r.frozen {
            return;
        }
        let v = if r.off {
            Verb::On
        } else if r.on {
            Verb::Sleep
        } else {
            Verb::Wake
        };
        self.verb(name, v, cx);
    }

    /// Space: the selected row's switch.
    pub(super) fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = self.selected.clone() {
            self.toggle_row(&n, cx);
        }
    }

    /// `w`, `s`, `r`, `p` on the selected row, as its page's keys do.
    pub(super) fn selected_verb(&mut self, verb: Verb, cx: &mut Context<Self>) {
        let Some(n) = self.selected.clone() else {
            return;
        };
        let Some(r) = all_rows(&self.home)
            .into_iter()
            .find(|r| r.name == n)
            .cloned()
        else {
            return;
        };
        if r.down || r.paused {
            return;
        }
        let verb = match (verb, r.kind == "task") {
            (Verb::Restart | Verb::Wake, true) => Verb::Task,
            (_, true) => return,
            (Verb::Wake, false) if r.off => Verb::On,
            (v, false) => v,
        };
        self.verb(&n, verb, cx);
    }

    /// Try again after a failed start.
    pub(crate) fn try_again(&mut self, cx: &mut Context<Self>) {
        let (_, f) = self.busy(cx);
        if f.is_some() && !self.home.running {
            self.stack_act(StackAction::UpLast, cx);
        }
    }

    /// Puts the failure away.
    pub(crate) fn dismiss_failure(&mut self, cx: &mut Context<Self>) {
        if let Some((a, k)) = self.acts(cx) {
            a.update(cx, |a, cx| a.dismiss_failure(&k, cx));
        }
    }
}

fn banner(t: &Tokens, tone: gpui::Hsla) -> gpui::Div {
    div()
        .mx(px(28.))
        .mb(px(12.))
        .flex()
        .items_start()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(mix(tone, t.panel, 50.))
        .bg(mix(tone, t.panel, 10.))
}

/// "Starting shop…" while the stack's action runs; the failure, kept until dismissed or tried again.
pub(super) fn banners(
    t: &Tokens,
    v: &OverviewView,
    cx: &mut Context<OverviewView>,
) -> Vec<AnyElement> {
    let (busy, failed) = v.busy(cx);
    if let Some(b) = busy {
        let mut word = b.word().to_owned();
        if let Some(first) = word.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        let ring = div()
            .mt(px(6.))
            .size(px(8.))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(t.cool);
        let words = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                div()
                    .font_weight(w(600.))
                    .text_color(t.bright)
                    .child(format!("{word} {}…", v.home.stack)),
            )
            .child(div().text_size(px(12.)).text_color(t.dim).child(b.detail()));
        return vec![
            banner(t, t.cool)
                .child(ring)
                .child(words)
                .into_any_element(),
        ];
    }
    let Some(f) = failed else { return Vec::new() };
    vec![failed_banner(t, v, &f, cx)]
}

/// The doze.hcl a failure is about, and its file name.
fn failure_file(v: &OverviewView, f: &Failure) -> (String, String) {
    let dir = v
        .stack
        .as_ref()
        .map(|st| st.dir.clone())
        .unwrap_or_default();
    let file = f
        .config
        .as_ref()
        .map_or_else(|| format!("{dir}/doze.hcl"), |c| c.file.clone());
    let name = std::path::Path::new(&file).file_name().map_or_else(
        || "doze.hcl".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    (file, name)
}

/// What doze printed, its last lines, in mono.
fn printed(t: &Tokens, f: &Failure) -> gpui::Div {
    div()
        .mt(px(6.))
        .p(px(10.))
        .rounded(px(8.))
        .bg(t.well)
        .border_1()
        .border_color(t.line)
        .font_family(MONO_FAMILY)
        .text_size(px(11.5))
        .line_height(px(17.))
        .text_color(t.dim)
        .children(f.lines.iter().rev().take(8).rev().map(|l| {
            div().child(if l.is_empty() {
                " ".to_owned()
            } else {
                l.clone()
            })
        }))
}

/// What was tried, what went wrong (or where doze.hcl is wrong), what doze printed.
/// "What doze printed" is a disclosure, closed at first (Go's `<details>`), so the banner stays short.
fn failure_words(
    t: &Tokens,
    (f, name, open): (&Failure, &str, bool),
    cx: &mut Context<OverviewView>,
) -> gpui::Div {
    let line = match &f.config {
        Some(c) if c.line > 0 => format!(
            "{name} has a mistake on line {}. Fix it in your editor, then try again; nothing was started.",
            c.line
        ),
        Some(_) => format!(
            "{name} has a mistake. Fix it in your editor, then try again; nothing was started."
        ),
        None => f.problem.clone(),
    };
    let tone = if f.config.is_some() { t.dim } else { t.bright };
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .font_weight(w(600.))
                .text_color(t.bright)
                .child(s(&f.what)),
        )
        .child(div().text_size(px(12.)).text_color(tone).child(line))
        .child(
            div()
                .id("fail-printed")
                .mt(px(4.))
                .text_size(px(11.5))
                .text_color(t.dim)
                .cursor_pointer()
                .hover(|d| d.text_color(t.text))
                .child(if open {
                    "▾ What doze printed"
                } else {
                    "▸ What doze printed"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.printed = !this.printed;
                    cx.notify();
                })),
        )
        .when(open, |d| d.child(printed(t, f)))
}

/// Edit doze.hcl (a mistake in it), Try again (a stopped stack), Copy, Dismiss.
fn failure_acts(
    t: &Tokens,
    (v, f): (&OverviewView, &Failure),
    (file, name): (String, String),
    cx: &mut Context<OverviewView>,
) -> gpui::Div {
    let mut acts = div().flex().flex_col().gap(px(6.)).flex_none();
    if f.config.is_some() {
        let open: crate::kit::Click =
            Box::new(move |_, _, cx| cx.open_url(&format!("file://{file}")));
        acts = acts.child(btn(
            t,
            "fail-edit",
            &format!("Edit {name}"),
            "",
            Btn::Primary,
            Some(open),
        ));
    }
    if !v.home.running {
        let kind = if f.config.is_some() {
            Btn::Plain
        } else {
            Btn::Primary
        };
        let again = Box::new(cx.listener(|this, _, _, cx| this.try_again(cx)));
        acts = acts.child(btn(t, "fail-again", "Try again", "", kind, Some(again)));
    }
    let lines = f.lines.join("\n");
    let copy = Box::new(cx.listener(move |this, _, _, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(lines.clone()));
        this.say("copied what doze printed", cx);
    }));
    let dismiss = Box::new(cx.listener(|this, _, _, cx| this.dismiss_failure(cx)));
    acts.child(btn(t, "fail-copy", "Copy", "", Btn::Quiet, Some(copy)))
        .child(btn(
            t,
            "fail-dismiss",
            "Dismiss",
            "",
            Btn::Quiet,
            Some(dismiss),
        ))
}

/// A stack action that did not work, kept on the page until dismissed or tried again (Go `FailedBanner`).
fn failed_banner(
    t: &Tokens,
    v: &OverviewView,
    f: &Failure,
    cx: &mut Context<OverviewView>,
) -> AnyElement {
    let (file, name) = failure_file(v, f);
    let words = failure_words(t, (f, &name, v.printed), cx);
    let acts = failure_acts(t, (v, f), (file, name), cx);
    let dot = div()
        .mt(px(6.))
        .size(px(8.))
        .flex_none()
        .rounded_full()
        .bg(t.bad);
    banner(t, t.bad)
        .child(dot)
        .child(words)
        .child(acts)
        .into_any_element()
}

/// Busy, failed and the verbs in flight are drawn from acts: the page redraws when they change.
pub(super) fn observe_acts(cx: &mut Context<OverviewView>) {
    let Some(a) = crate::act::acts(cx) else {
        return;
    };
    cx.observe(&a, |this, a, cx| {
        this.doing = a
            .read(cx)
            .doing
            .iter()
            .filter(|((k, _), _)| this.key.as_ref() == Some(k))
            .map(|((_, n), v)| (n.clone(), *v))
            .collect();
        this.refresh(cx);
        cx.notify();
    })
    .detach();
}

/// What a row says while a verb on it is in flight: "sleeping…", in the waking blue, the reason cleared; so the words
/// never contradict the switch, which already shows where it is going.
fn busy_word(v: Verb) -> &'static str {
    match v {
        Verb::Sleep => "sleeping…",
        Verb::Wake | Verb::On | Verb::Up => "waking…",
        Verb::Restart => "restarting…",
        Verb::Off => "turning off…",
        Verb::Pin => "pinning…",
        Verb::Task => "running…",
        Verb::Reset => "resetting…",
        Verb::Sync => "applying…",
    }
}

/// The rows and the pane with the verbs in flight marked.
pub(super) fn mark_doing(
    mut h: doze_desk::overview::Home,
    mut p: Option<doze_desk::overview::Pane>,
    doing: &std::collections::HashMap<String, Verb>,
) -> (doze_desk::overview::Home, Option<doze_desk::overview::Pane>) {
    if doing.is_empty() {
        return (h, p);
    }
    let look = |v: Verb| doze_desk::overview::Look {
        word: busy_word(v),
        glyph: "◌",
        tone: "cool",
    };
    for r in h.groups.iter_mut().flat_map(|g| g.rows.iter_mut()) {
        if let Some(v) = doing.get(&r.name) {
            r.look = look(*v);
            r.because.clear();
        }
    }
    if let Some(p) = p.as_mut()
        && let Some(v) = doing.get(&p.name)
    {
        p.look = look(*v);
        p.because.clear();
    }
    (h, p)
}
