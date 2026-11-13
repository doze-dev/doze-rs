//! How ⌘K is drawn: the box with its crumbs, the sections and their rows (glyph, the name with its matched letters
//! marked, the words, and at the right the key, the command line of the highlighted row, "tab its commands" or
//! "Copied ✓"), the no-match line, and the keys at the foot.

use doze_desk::palette::{Nothing, Row};
use gpui::{
    AnyElement, Context, Hsla, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};
use gpui_component::input::Input;

use super::palette::Palette;
use crate::actions::{BackOut, Confirm, FirstAct, MoveDown, MoveUp, ScopeIn, ScopeOut};
use crate::theme::{Tokens, tokens};
use crate::widgets::{MONO_FAMILY, UI_FAMILY, icon, kbd_chip, tracked, w};

/// A palette tone's colour (Go's dot classes `g y r b`, and `dim`, `acc`).
fn ink(t: &Tokens, tone: &str) -> Hsla {
    match tone {
        "g" => t.good,
        "y" => t.warn,
        "r" => t.bad,
        "b" => t.cool,
        "acc" => t.accent,
        "dim" => t.dim,
        _ => t.text,
    }
}

fn glyph(t: &Tokens, r: &Row) -> AnyElement {
    let c = ink(t, r.tone);
    let d = div().w(px(20.)).flex_none().flex().justify_center();
    if r.glyph == "dot" {
        return d
            .child(div().size(px(8.)).rounded_full().bg(c))
            .into_any_element();
    }
    // A two-character glyph (⌘O, ›_) is set smaller, so it keeps to the column.
    let size = if r.glyph.chars().count() > 1 {
        10.5
    } else {
        12.5
    };
    d.text_color(c)
        .text_size(px(size))
        .child(SharedString::from(r.glyph))
        .into_any_element()
}

/// The label, its matched letters in the accent.
fn label(t: &Tokens, text: &str, at: &[usize]) -> AnyElement {
    let mut runs: Vec<(String, bool)> = Vec::new();
    for (i, ch) in text.chars().enumerate() {
        let m = at.contains(&i);
        match runs.last_mut() {
            Some((s, was)) if *was == m => s.push(ch),
            _ => runs.push((ch.to_string(), m)),
        }
    }
    div()
        .flex()
        .flex_none()
        .font_weight(w(600.))
        .text_color(t.bright)
        .children(runs.into_iter().map(|(s, m)| {
            let d = div().child(SharedString::from(s));
            if m { d.text_color(t.accent) } else { d }
        }))
        .into_any_element()
}

/// The right side of a row: "Copied ✓", "tab its commands", its command line, or its key.
fn right(t: &Tokens, r: &Row, on: bool, copied: bool) -> AnyElement {
    let small = |text: String, c: Hsla| {
        div()
            .flex_none()
            .text_size(px(11.5))
            .text_color(c)
            .child(SharedString::from(text))
    };
    if copied {
        return small("Copied ✓".into(), t.good)
            .font_weight(w(600.))
            .into_any_element();
    }
    if on && r.enter.is_some() {
        return div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(5.))
            .text_size(px(11.5))
            .text_color(t.dim)
            .child(kbd_chip(t, "tab", 10., 600., 5.))
            .child("its commands")
            .into_any_element();
    }
    if on && !r.cli.is_empty() && !r.doing.ask {
        return small(r.cli.clone(), t.dim)
            .font_family(MONO_FAMILY)
            .into_any_element();
    }
    small(r.key.clone(), if on { t.text } else { t.dim })
        .font_family(MONO_FAMILY)
        .text_size(px(11.))
        .into_any_element()
}

fn item(t: &Tokens, p: &Palette, r: &Row, i: usize, cx: &mut Context<Palette>) -> AnyElement {
    let on = i == p.on;
    let copied = p.copied.as_deref() == Some(r.label.as_str());
    let note = if r.bad {
        t.warn
    } else if on {
        t.text
    } else {
        t.dim
    };
    div()
        .id(("pal", i))
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(36.))
        .px(px(16.))
        .text_size(px(13.))
        .when(on, |d| d.bg(t.pick))
        .hover(|d| d.bg(t.sel))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| this.click(i, window, cx)))
        .child(glyph(t, r))
        .child(label(t, &r.label, &r.at))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(12.))
                .text_color(note)
                .child(SharedString::from(r.note.clone())),
        )
        .child(right(t, r, on, copied))
        .into_any_element()
}

/// "Nothing called x in shop or acme. Closest: cache (redis, in shop)."
fn nothing(t: &Tokens, n: &Nothing, cx: &mut Context<Palette>) -> AnyElement {
    let b = |s: &str| {
        div()
            .font_weight(w(600.))
            .text_color(t.bright)
            .child(SharedString::from(s.to_owned()))
    };
    let mut words = div()
        .flex()
        .flex_1()
        .min_w_0()
        .flex_wrap()
        .gap_x(px(4.))
        .text_color(t.text)
        .child("Nothing called")
        .child(b(&n.name))
        .child(SharedString::from(format!("in {}.", n.r#where)));
    if !n.closest.is_empty() {
        words = words
            .child("Closest:")
            .child(b(&n.closest))
            .child(SharedString::from(format!("({}).", n.why)));
    }
    div()
        .id("pal-nothing")
        .flex()
        .items_center()
        .gap(px(10.))
        .min_h(px(40.))
        .px(px(16.))
        .bg(t.pick)
        .cursor_pointer()
        .on_click(cx.listener(|this, _, window, cx| this.choose(window, cx)))
        .child(div().w(px(16.)).text_color(t.dim).child("?"))
        .child(words)
        .child(
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(t.text)
                .child(SharedString::from(n.key.clone())),
        )
        .into_any_element()
}

fn foot_key(t: &Tokens, k: &str, what: &str) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(div().mr(px(4.)).child(kbd_chip(t, k, 10., 600., 5.)))
        .child(SharedString::from(what.to_owned()))
        .into_any_element()
}

impl Palette {
    /// The keys, and what to try (or where tab narrowed to).
    fn foot(&self, t: &Tokens) -> impl IntoElement {
        let last = self.model.crumbs.last().cloned().unwrap_or_default();
        let acts = self.row().and_then(|r| r.acts.first()).map(|a| a.label);
        let mut f = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .h(px(32.))
            .px(px(16.))
            .border_t_1()
            .border_color(t.line)
            .text_size(px(11.5))
            .text_color(t.dim)
            .whitespace_nowrap()
            .child(foot_key(t, "↑↓", "move"))
            .child(foot_key(t, "⏎", "do it"));
        f = if self.scope.is_some() {
            f.child(foot_key(t, "⌫", "step out"))
        } else {
            f.child(foot_key(t, "tab", "into a stack or service"))
        };
        if let Some(a) = acts {
            f = f.child(foot_key(t, "⌘⏎", a));
        }
        let note = if self.scope.is_some() {
            format!("the commands for {last}")
        } else if acts.is_some() {
            // The ⌘⏎ hint takes the room the examples had.
            String::new()
        } else {
            "try: api restart · db psql · api timeout · theme teal".into()
        };
        f.child(div().flex_1())
            .child(div().flex_none().child(SharedString::from(note)))
    }

    /// The sections and their rows, then the no-match line.
    fn body(&self, t: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div()
            .id("pal-body")
            .flex()
            .flex_col()
            .max_h(px(440.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        let mut i = 0;
        for s in &self.model.sections {
            body = body.child(
                div()
                    .px(px(16.))
                    .pt(px(8.))
                    .pb(px(2.))
                    .child(tracked(&s.title, 11., 600., t.dim, 0.66)),
            );
            for r in &s.rows {
                body = body.child(item(t, self, r, i, cx));
                i += 1;
            }
        }
        if let Some(n) = &self.model.nothing {
            body = body.child(nothing(t, n, cx));
        } else if i == 0 {
            body = body.child(
                div()
                    .px(px(16.))
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .text_color(t.dim)
                    .child("Nothing to do here"),
            );
        }
        body.into_any_element()
    }

    /// The box: the glass, the crumbs (each steps out), the input.
    fn head(&self, t: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let crumbs = if self.scope.is_some() {
            self.model.crumbs.clone()
        } else {
            Vec::new()
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(46.))
            .px(px(16.))
            .border_b_1()
            .border_color(t.line)
            .child(icon("icons/search.svg", 18., t.dim))
            .children(crumbs.into_iter().enumerate().map(|(i, c)| {
                div()
                    .id(("crumb", i))
                    .flex_none()
                    .px(px(8.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .bg(t.sel)
                    .text_size(px(12.5))
                    .font_weight(w(600.))
                    .text_color(t.bright)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scope_out(cx);
                    }))
                    .child(SharedString::from(format!("{c} ›")))
            }))
            .child(
                div().flex_1().child(
                    Input::new(&self.input)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .px_0()
                        .text_size(px(15.))
                        .text_color(t.bright),
                ),
            )
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let last = self.model.crumbs.last().cloned();
        let hint = match (&self.scope, last) {
            (Some(_), Some(c)) => format!("What to do with {c}…"),
            _ => "A service, a stack, or what to do…".into(),
        };
        if self.hint != hint {
            self.hint.clone_from(&hint);
            self.input
                .update(cx, |i, cx| i.set_placeholder(hint, window, cx));
        }
        let body = self.body(&t, cx);
        div()
            .key_context("Palette")
            .w(px(600.))
            .font_family(UI_FAMILY)
            .text_size(px(13.))
            .line_height(px(20.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(t.line2)
            .bg(t.panel)
            .shadow(vec![super::shadow(&t)])
            .on_action(cx.listener(|this, _: &ScopeIn, window, cx| {
                if !this.scope_in(window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ScopeOut, _, cx| {
                this.scope_out(cx);
            }))
            .on_action(cx.listener(|this, _: &BackOut, _, cx| {
                // Not on an empty box: the box's own ⌫.
                if !this.backspace(cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &FirstAct, _, cx| this.first_act(cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.choose(window, cx)))
            .child(self.head(&t, cx))
            .child(body)
            .child(self.foot(&t))
    }
}
