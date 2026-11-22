//! How the help sheet's keys are drawn: a context's keys in their groups beside Everywhere's (set a step smaller, so
//! the page's own read first), the words as a glossary, and what a filter found. Labels wrap; nothing is cut.

use doze_display::Look;
use gpui::{
    AnyElement, HighlightStyle, Hsla, IntoElement, SharedString, StyledText, div, prelude::*, px,
};

use super::model::{self, Found, Group, Line, Section, Word};
use crate::actions::Scope;
use crate::theme::{Tokens, ty};
use crate::widgets::{kbd_chip, tracked};

/// The keys' column, wide enough for two chips side by side (`↑↓←→ hjkl`); more wrap under.
const KEYS_W: f32 = 92.;

/// `text` in runs: each part in its colour, the first match of `q` in each part in the accent.
fn runs(parts: &[(&str, Hsla, gpui::FontWeight)], q: &str, accent: Hsla) -> StyledText {
    let mut text = String::new();
    let mut styles: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let style = |c: Hsla, w: gpui::FontWeight| HighlightStyle {
        color: Some(c),
        font_weight: Some(w),
        ..Default::default()
    };
    for (part, c, w) in parts {
        let at = text.len();
        text.push_str(part);
        match model::hit(part, q) {
            Some(r) => {
                let (a, b) = (at + r.start, at + r.end);
                if a > at {
                    styles.push((at..a, style(*c, *w)));
                }
                styles.push((a..b, style(accent, *w)));
                if text.len() > b {
                    styles.push((b..text.len(), style(*c, *w)));
                }
            }
            None if !part.is_empty() => styles.push((at..text.len(), style(*c, *w))),
            None => {}
        }
    }
    StyledText::new(text).with_highlights(styles)
}

/// A line: its keys, then what it does and the words after, wrapping in its column; `small` a step down.
fn line(t: &Tokens, l: &Line, q: &str, small: bool) -> AnyElement {
    let mut parts = vec![(l.label.as_str(), t.bright, ty::MEDIUM)];
    if !l.what.is_empty() {
        parts.push((" · ", t.dim, ty::REGULAR));
        parts.push((l.what, t.dim, ty::REGULAR));
    }
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .py(px(if small { 2. } else { 4. }))
        .child(
            div()
                .w(px(KEYS_W))
                .flex_none()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .pt(px(1.))
                .children(
                    l.keys
                        .iter()
                        .map(|k| kbd_chip(t, k, ty::XS, ty::SEMIBOLD, 5.).into_any_element()),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(if small { ty::SM } else { ty::BASE }))
                .line_height(px(if small { ty::LH_18 } else { ty::LH }))
                .child(runs(&parts, q, t.accent)),
        )
        .into_any_element()
}

/// A small group heading (`MOVE`, `ZOOM`).
fn small_head(t: &Tokens, title: &str) -> impl IntoElement {
    div()
        .pt(px(2.))
        .pb(px(3.))
        .child(tracked(title, ty::XS, ty::HEADING, t.dim, 0.66))
}

fn group(t: &Tokens, g: &Group, small: bool) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .pb(px(if small { 8. } else { 14. }))
        .children(g.title.map(|h| small_head(t, h)))
        .children(g.lines.iter().map(|l| line(t, l, "", small)))
        .into_any_element()
}

/// A column: its heading (the context's), then its blocks.
fn column(t: &Tokens, title: &str, blocks: Vec<AnyElement>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .pb(px(6.))
                .text_size(px(ty::MD))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                // An untitled column keeps the heading's height, so its groups line up with the first column's.
                .child(SharedString::from(if title.is_empty() {
                    "\u{a0}".to_owned()
                } else {
                    title.to_owned()
                })),
        )
        .children(blocks)
}

/// Blocks of about `weights` lines each, split in two runs (kept in order) whose sums are as close as can be.
fn halves<T>(blocks: Vec<(usize, T)>) -> (Vec<T>, Vec<T>) {
    let total: usize = blocks.iter().map(|b| b.0).sum();
    let mut run = 0;
    let mut cut = blocks.len();
    let mut best = usize::MAX;
    for (i, (w, _)) in blocks.iter().enumerate() {
        let gap = run.max(total - run) - run.min(total - run);
        if gap < best {
            best = gap;
            cut = i;
        }
        run += w;
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (i, (_, b)) in blocks.into_iter().enumerate() {
        if i < cut { left.push(b) } else { right.push(b) }
    }
    (left, right)
}

/// Two columns side by side, or the first above the second.
fn pair(two: bool, a: gpui::Div, b: gpui::Div) -> AnyElement {
    let d = div().flex().gap(px(36.));
    let d = if two { d } else { d.flex_col().gap(px(10.)) };
    d.child(a).child(b).into_any_element()
}

/// A section, as the rail picks it: a context's keys with Everywhere's beside them, Everywhere's alone in two
/// columns, or the words.
pub(super) fn section(t: &Tokens, s: Section, two: bool) -> AnyElement {
    let blocks = |sc: Scope, small: bool| -> Vec<(usize, AnyElement)> {
        model::grouped(sc)
            .iter()
            .map(|g| (g.lines.len() + 1, group(t, g, small)))
            .collect()
    };
    let only = |b: Vec<(usize, AnyElement)>| b.into_iter().map(|b| b.1).collect();
    match s {
        Section::Words => words(t, model::words(), "", two),
        Section::Keys(Scope::Global) if two => {
            let (a, b) = halves(blocks(Scope::Global, false));
            pair(true, column(t, s.title(), a), column(t, "", b))
        }
        Section::Keys(Scope::Global) => {
            column(t, s.title(), only(blocks(Scope::Global, false))).into_any_element()
        }
        Section::Keys(sc) => pair(
            two,
            column(t, s.title(), only(blocks(sc, false))),
            column(
                t,
                Section::Keys(Scope::Global).title(),
                only(blocks(Scope::Global, true)),
            ),
        ),
    }
}

/// One word: its mark where the app draws one, the word in its tone, what it means under it or beside it.
fn word(t: &Tokens, (w, tone, means): &Word, q: &str) -> AnyElement {
    let ink = crate::kit::tone(t, tone);
    let mark = Look::of_word(w).map(|l| crate::instance::ui::state_mark(t, Some(l), false, 8.));
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .py(px(7.))
        .child(
            div()
                .w(px(112.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(ty::LH))
                .child(
                    div()
                        .w(px(10.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .children(mark),
                )
                .child(div().child(runs(&[(w, ink, ty::SEMIBOLD)], q, t.accent))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(runs(&[(means, t.text, ty::REGULAR)], q, t.accent)),
        )
        .into_any_element()
}

/// The words as a glossary, in two columns when there is room.
fn words(t: &Tokens, ws: &[Word], q: &str, two: bool) -> AnyElement {
    let all: Vec<(usize, AnyElement)> = ws.iter().map(|w| (1, word(t, w, q))).collect();
    if !two {
        return column(t, "The words", all.into_iter().map(|b| b.1).collect()).into_any_element();
    }
    let (a, b) = halves(all);
    pair(true, column(t, "The words", a), column(t, "", b))
}

/// What the filter found: each context's matches under its name, then the words.
pub(super) fn found(t: &Tokens, f: &Found, q: &str, two: bool) -> AnyElement {
    let ql = q.to_lowercase();
    if f.is_empty() {
        return div()
            .pt(px(24.))
            .text_color(t.dim)
            .child(SharedString::from(format!("No key or word matches “{q}”.")))
            .into_any_element();
    }
    let mut blocks: Vec<(usize, AnyElement)> = f
        .keys
        .iter()
        .map(|(sc, ls)| {
            let el = div()
                .flex()
                .flex_col()
                .pb(px(14.))
                .child(small_head(t, sc.title()))
                .children(ls.iter().map(|l| line(t, l, &ql, false)))
                .into_any_element();
            (ls.len() + 1, el)
        })
        .collect();
    if !f.words.is_empty() {
        let el = div()
            .flex()
            .flex_col()
            .child(small_head(t, "The words"))
            .children(f.words.iter().map(|w| word(t, w, &ql)))
            .into_any_element();
        blocks.push((f.words.len() * 2 + 1, el));
    }
    let n: usize = f.keys.iter().map(|(_, l)| l.len()).sum::<usize>() + f.words.len();
    let title = format!("{n} {} for “{q}”", if n == 1 { "match" } else { "matches" });
    if !two {
        return column(t, &title, blocks.into_iter().map(|b| b.1).collect()).into_any_element();
    }
    let (a, b) = halves(blocks);
    pair(true, column(t, &title, a), column(t, "", b))
}
