//! ⌘K: the command palette, drawn as the board drew it (the spike matched it to the React page at 2.2/255). Its "Do"
//! rows are the action table's palette rows, so ⌘K offers exactly what the menus and keys do; its "Cards" rows jump to
//! a card on the Map. The full palette model (scopes, did-you-mean, the CLI form) is M2's.

use doze_display::{fuzzy_match, fuzzy_score};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::actions::{self, Spec};
use crate::theme::{Tokens, tokens};
use crate::widgets::{MONO_FAMILY, UI_FAMILY, icon, kbd_chip, tracked, w};

/// What a row does.
#[derive(Clone, Copy)]
pub(crate) enum Pick {
    Act(&'static Spec),
    Card(usize),
}

/// One row.
#[derive(Clone)]
struct Row {
    label: SharedString,
    note: SharedString,
    key: Option<String>,
    pick: Pick,
    soon: bool,
}

/// What the palette tells its owner.
pub(crate) enum PaletteEvent {
    Chose(Pick),
}

/// The palette.
pub(crate) struct Palette {
    pub(crate) input: Entity<InputState>,
    rows: Vec<Row>,
    cards: Vec<String>,
    /// The rows shown: (row, matched positions).
    shown: Vec<(usize, Vec<usize>)>,
    on: usize,
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Palette {
    /// A palette over the action table and the stack's cards (by name, with their words).
    pub(crate) fn new(
        cards: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search or do: a card, a view, a theme…")
        });
        cx.subscribe(&input, |this: &mut Palette, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let q = input.read(cx).value().to_string();
                this.filter(&q);
                cx.notify();
            }
        })
        .detach();
        input.update(cx, |i, cx| i.focus(window, cx));
        let mut rows: Vec<Row> = actions::palette()
            .map(|s| Row {
                label: s.label.into(),
                note: s.what.into(),
                key: actions::chip(s),
                pick: Pick::Act(s),
                soon: !s.built,
            })
            .collect();
        let names: Vec<String> = cards.iter().map(|c| c.0.clone()).collect();
        rows.extend(cards.into_iter().enumerate().map(|(i, (name, note))| Row {
            label: name.into(),
            note: note.into(),
            key: None,
            pick: Pick::Card(i),
            soon: false,
        }));
        let mut p = Palette {
            input,
            rows,
            cards: names,
            shown: Vec::new(),
            on: 0,
        };
        p.filter("");
        p
    }

    /// The card name of a [`Pick::Card`].
    pub(crate) fn card(&self, i: usize) -> Option<&str> {
        self.cards.get(i).map(String::as_str)
    }

    fn filter(&mut self, q: &str) {
        let q = q.trim();
        if q.is_empty() {
            // Nothing typed: a few things it does now, then the stack's cards. What comes later waits for a query.
            let acts = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| matches!(r.pick, Pick::Act(_)) && !r.soon)
                .take(4);
            let cards = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| matches!(r.pick, Pick::Card(_)))
                .take(5);
            self.shown = acts.chain(cards).map(|(i, _)| (i, Vec::new())).collect();
            self.on = 0;
            return;
        }
        // Typed: best match first, what the app does now before what comes later.
        let mut hits: Vec<(bool, usize, usize, Vec<usize>)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let at = fuzzy_match(q, &r.label)?;
                Some((r.soon, fuzzy_score(&at), i, at))
            })
            .collect();
        hits.sort_by_key(|h| (h.0, h.1, h.2));
        self.shown = hits
            .into_iter()
            .take(9)
            .map(|(_, _, i, at)| (i, at))
            .collect();
        self.on = 0;
    }

    /// Moves the highlight.
    pub(crate) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.shown.len();
        if n > 0 {
            self.on = self.on.saturating_add_signed(by).min(n - 1);
            cx.notify();
        }
    }

    /// Chooses the highlighted row.
    pub(crate) fn choose(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self
            .shown
            .get(self.on)
            .and_then(|(i, _)| self.rows.get(*i))
            .filter(|r| !r.soon)
        {
            cx.emit(PaletteEvent::Chose(r.pick));
        }
    }
}

fn label(t: &Tokens, text: &str, at: &[usize], soon: bool) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .text_color(if soon { t.dim } else { t.bright })
        .font_weight(w(600.))
        .children(text.chars().enumerate().map(|(i, ch)| {
            let d = div().child(SharedString::from(ch.to_string()));
            if at.contains(&i) {
                d.text_color(t.accent)
            } else {
                d
            }
        }))
        .into_any_element()
}

fn item(
    t: &Tokens,
    r: &Row,
    at: &[usize],
    on: bool,
    ix: usize,
    cx: &mut Context<Palette>,
) -> AnyElement {
    let note_color = if on { t.text } else { t.dim };
    let right: AnyElement = match (&r.key, r.soon) {
        (_, true) => div()
            .flex_none()
            .text_size(px(11.))
            .text_color(t.dim)
            .child("soon")
            .into_any_element(),
        (Some(k), false) => kbd_chip(t, k, 10., 600., 5.).into_any_element(),
        (None, false) => div()
            .flex_none()
            .font_family(MONO_FAMILY)
            .text_size(px(11.))
            .text_color(note_color)
            .child("⏎ show")
            .into_any_element(),
    };
    let glyph = match r.pick {
        Pick::Act(_) => div()
            .w(px(13.))
            .text_color(t.dim)
            .child("›")
            .into_any_element(),
        Pick::Card(_) => div()
            .size(px(8.))
            .mx(px(2.5))
            .rounded_full()
            .bg(t.accent)
            .into_any_element(),
    };
    div()
        .id(("pal", ix))
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(36.))
        .px(px(16.))
        .text_size(px(13.))
        .text_color(if on { t.bright } else { t.text })
        .when(on, |d| d.bg(t.pick))
        .hover(|d| d.bg(t.sel))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.on = ix;
            this.choose(cx);
        }))
        .child(glyph)
        .child(label(t, &r.label, at, r.soon))
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(12.))
                .text_color(note_color)
                .child(r.note.clone()),
        )
        .child(right)
        .into_any_element()
}

fn foot_key(t: &Tokens, k: &str, what: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(div().mr(px(4.)).child(kbd_chip(t, k, 10., 600., 5.)))
        .child(SharedString::from(what.to_string()))
}

/// The foot: the keys, and what to try.
fn foot(t: &Tokens) -> impl IntoElement {
    div()
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
        .child(foot_key(t, "⏎", "do it"))
        .child(foot_key(t, "esc", "close"))
        .child(div().flex_1())
        .child(div().flex_none().child("try: teal · fit · find · dark"))
}

impl Palette {
    /// The rows shown, under their headings.
    fn body(&self, t: &Tokens, cx: &mut Context<Self>) -> gpui::Div {
        let mut body = div().flex().flex_col();
        let mut last: Option<bool> = None;
        let shown = self.shown.clone();
        for (k, (i, at)) in shown.iter().enumerate() {
            let Some(r) = self.rows.get(*i).cloned() else {
                continue;
            };
            let is_act = matches!(r.pick, Pick::Act(_));
            if last != Some(is_act) {
                let title = if is_act { "Do" } else { "Cards on the map" };
                body = body.child(
                    div()
                        .px(px(16.))
                        .pt(px(8.))
                        .pb(px(2.))
                        .child(tracked(title, 11., 600., t.dim, 0.66)),
                );
                last = Some(is_act);
            }
            body = body.child(item(t, &r, at, k == self.on, k, cx));
        }
        if self.shown.is_empty() {
            body = body.child(
                div()
                    .px(px(16.))
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .text_color(t.dim)
                    .child("Nothing by that name"),
            );
        }
        body
    }
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let body = self.body(&t, cx);
        div()
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(46.))
                    .px(px(16.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(icon("icons/search.svg", 18., t.dim))
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
                    ),
            )
            .child(body)
            .child(foot(&t))
    }
}
