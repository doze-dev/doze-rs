//! A folder with no doze.hcl yet (the OpenFolder board; Go `pages/empty.tsx`): what doze found there, one line each,
//! and the three ways to start, the same three as a bare `doze` in an empty folder. Nothing starts until one is chosen,
//! and nothing runs until the stack is brought up. Keys (context `Empty`): ←→ (or ↑↓) move, ⏎ chooses, 1–3 choose that
//! one, Esc leaves without writing anything. Reached by Open folder ⌘O, New stack ⌘N, or a folder dropped on the window.

use std::path::PathBuf;

use doze_desk::act::{Empty, Way, detect, empty_of};
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Window, div,
    prelude::*, px,
};

use crate::act::{Acts, Ask};
use crate::actions::{Confirm, MoveLeft, MoveRight, Way1, Way2, Way3};
use crate::kit::{keys_bar, mono, s};
use crate::theme::{Tokens, mix, tokens};
use crate::widgets::{MONO_FAMILY, tracked, w};

/// The page.
pub(crate) struct EmptyView {
    pub(crate) dir: PathBuf,
    pub(crate) page: Empty,
    acts: Entity<Acts>,
    at: usize,
    pub(crate) focus: FocusHandle,
}

impl Focusable for EmptyView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EmptyView {
    pub(crate) fn new(dir: PathBuf, acts: Entity<Acts>, cx: &mut Context<Self>) -> EmptyView {
        cx.observe(&acts, |_, _, cx| cx.notify()).detach();
        let page = empty_of(&dir, &Default::default());
        let d = dir.clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { detect(&d) })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.page = empty_of(&v.dir, &found);
                // The first way that can work here is the one ⏎ takes.
                v.at = v.page.ways.iter().position(|w| !w.off).unwrap_or(0);
                cx.notify();
            });
        })
        .detach();
        EmptyView {
            dir,
            page,
            acts,
            at: 0,
            focus: cx.focus_handle(),
        }
    }

    /// The way being written, if one is.
    fn writing(&self, cx: &App) -> Option<String> {
        self.acts
            .read(cx)
            .writing
            .as_ref()
            .filter(|(d, _)| *d == self.dir)
            .map(|(_, w)| w.clone())
    }

    fn choose(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(way) = self.page.ways.get(i).cloned() else {
            return;
        };
        self.at = i;
        cx.notify();
        if way.off || self.writing(cx).is_some() {
            return;
        }
        let dir = self.dir.clone();
        let add = self.page.add.clone();
        self.acts.update(cx, |a, cx| {
            if way.action == "add" {
                cx.emit(crate::act::ActsEvent::Ask(Ask {
                    key: None,
                    dialog: add,
                }));
            } else {
                a.setup(&dir, way.action, String::new(), cx);
            }
        });
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.page.ways.len().max(1);
        self.at = self.at.saturating_add_signed(by).min(n - 1);
        cx.notify();
    }
}

fn found(t: &Tokens, e: &Empty) -> Option<AnyElement> {
    if e.items.is_empty() {
        return None;
    }
    let cols = |a: AnyElement, b: AnyElement, c: AnyElement| {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .child(div().w(px(140.)).flex_none().child(a))
            .child(div().flex_1().min_w_0().overflow_hidden().child(b))
            .child(div().w(px(180.)).flex_none().overflow_hidden().child(c))
    };
    let head = cols(
        tracked("Found", 11., 600., t.dim, 0.66).into_any_element(),
        tracked("Where", 11., 600., t.dim, 0.66).into_any_element(),
        tracked("Becomes", 11., 600., t.dim, 0.66).into_any_element(),
    )
    .h(px(34.))
    .bg(t.well);
    let rows = e.items.iter().map(|it| {
        cols(
            mono(s(&it.name), 12., t.bright)
                .font_weight(w(600.))
                .into_any_element(),
            div()
                .text_color(t.dim)
                .whitespace_nowrap()
                .text_ellipsis()
                .child(s(&it.where_))
                .into_any_element(),
            div()
                .font_family(MONO_FAMILY)
                .text_size(px(12.))
                .text_color(t.dim)
                .whitespace_nowrap()
                .text_ellipsis()
                .child(s(&it.becomes))
                .into_any_element(),
        )
        .h(px(37.))
        .border_t_1()
        .border_color(t.line)
    });
    Some(
        div()
            .mx(px(28.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(t.line)
            .bg(t.panel)
            .overflow_hidden()
            .child(head)
            .children(rows)
            .into_any_element(),
    )
}

impl EmptyView {
    fn tile(
        &self,
        t: &Tokens,
        i: usize,
        way: &Way,
        writing: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sel = i == self.at;
        let busy = writing == Some(way.action);
        let dim = way.off || (writing.is_some() && !busy);
        let num = div()
            .size(px(22.))
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(t.line2)
            .flex()
            .items_center()
            .justify_center()
            .child(mono(way.n.to_string(), 11., t.dim).font_weight(w(600.)));
        div()
            .id(("way", i))
            .flex_1()
            .min_w_0()
            .min_h(px(96.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(18.))
            .pt(px(16.))
            .pb(px(14.))
            .rounded(px(12.))
            .border_1()
            .border_color(if sel && !way.off { t.line2 } else { t.line })
            .when(way.off, |d| d.border_dashed())
            .when(!way.off, |d| {
                d.bg(if sel {
                    mix(t.accent, t.panel, 7.)
                } else {
                    t.panel
                })
                .cursor_pointer()
            })
            .when(dim, |d| d.opacity(0.55))
            .child(
                div().flex().items_center().gap(px(8.)).child(num).child(
                    div()
                        .text_size(px(15.))
                        .font_weight(w(650.))
                        .text_color(if way.off { t.dim } else { t.bright })
                        .child(way.title),
                ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(if busy { t.cool } else { t.dim })
                    .child(if busy {
                        "writing doze.hcl…"
                    } else {
                        way.what
                    }),
            )
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.at != i {
                    this.at = i;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(i, cx)))
            .into_any_element()
    }
}

impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let writing = self.writing(cx);
        let e = self.page.clone();
        let tiles: Vec<AnyElement> = e
            .ways
            .iter()
            .enumerate()
            .map(|(i, way)| self.tile(&t, i, way, writing.as_deref(), cx))
            .collect();
        let keys = [
            ("1-3".to_owned(), "pick"),
            ("⏎".to_owned(), "choose"),
            ("esc".to_owned(), "close without writing anything"),
        ];
        div()
            .key_context("Empty")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.choose(this.at, cx)))
            .on_action(cx.listener(|this, _: &Way1, _, cx| this.choose(0, cx)))
            .on_action(cx.listener(|this, _: &Way2, _, cx| this.choose(1, cx)))
            .on_action(cx.listener(|this, _: &Way3, _, cx| this.choose(2, cx)))
            .flex()
            .flex_col()
            .size_full()
            .child(head(&t, &e))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(banner(&t, &e))
                    .children(found(&t, &e))
                    .child(div().mx(px(28.)).flex().gap(px(14.)).children(tiles))
                    .child(model(&t)),
            )
            .child(keys_bar(
                &t,
                &keys,
                "from Home: Open folder, a drop, or doze open in a terminal",
            ))
    }
}

/// `.head`: the folder's name, its path, "no doze.hcl yet".
fn head(t: &Tokens, e: &Empty) -> impl IntoElement {
    let path = div()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_color(t.dim)
        .child(format!("{} · no doze.hcl yet", e.dir));
    div()
        .flex()
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
                .child(s(&e.name)),
        )
        .child(path)
}

/// The cool banner: nothing has started, and what doze found.
fn banner(t: &Tokens, e: &Empty) -> impl IntoElement {
    let lead = if e.items.is_empty() {
        "Nothing has started. doze read the folder and found nothing it recognises."
    } else {
        "Nothing has started. doze read the folder and found this."
    };
    let words = div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(div().font_weight(w(600.)).text_color(t.bright).child(lead))
        .child(div().text_size(px(12.)).text_color(t.dim).child("Pick one of the three ways below; each writes a doze.hcl you can open in your editor. Nothing runs until you bring the stack up."));
    div()
        .mx(px(28.))
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(mix(t.cool, t.panel, 50.))
        .bg(mix(t.cool, t.panel, 10.))
        .child(div().size(px(8.)).flex_none().rounded_full().bg(t.cool))
        .child(words)
}

/// `.model`: the same three as a bare doze; the file is yours.
fn model(t: &Tokens) -> impl IntoElement {
    div()
        .mx(px(28.))
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .text_size(px(12.5))
        .text_color(t.dim)
        .child("The same three choices as")
        .child(mono("doze", 12., t.dim))
        .child("run in a terminal in this folder. doze never edits")
        .child(mono("doze.hcl", 12., t.dim))
        .child("behind your back: after this, the file is yours.")
}
