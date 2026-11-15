//! Needs you, the panel under the bell (the NeedsYouPanel and BellEmpty boards, Go `components/bell.tsx`): what is
//! broken now, then what is worth doing, each with its fixes, Show, Not now and its age; an item whose cause went away
//! says "fixed ✓" for a moment. Calm, it says so and what would show here. It owns the keys while open: ↑↓ move, ⏎
//! the first fix, ⌘⏎ the page that explains it, n not now, esc closes.

use std::time::SystemTime;

use doze_desk::attention::{Attention, Fix, FixKind, Item};
use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Window, div, prelude::*, px,
};

use crate::actions::{BellFix, BellNotNow, BellShow, MoveDown, MoveUp};
use crate::kit::{Btn, btn};
use crate::outward::Probe as _;
use crate::theme::{Tokens, tokens, ty};
use crate::widgets::{MONO_FAMILY, UI_FAMILY, kbd_chip, tracked};

/// What the panel asks its owner to do.
pub(crate) enum BellEvent {
    Fix(Item, Fix),
    Show(Item),
    NotNow(Item),
}

/// The panel.
pub(crate) struct Bell {
    pub(crate) list: Attention,
    pub(crate) now: SystemTime,
    /// The highlighted item, among those not fixed.
    on: usize,
    focus: FocusHandle,
}

impl EventEmitter<BellEvent> for Bell {}

impl Focusable for Bell {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

/// An age as the panel writes it: now, 4m, 2h, 3d.
fn age(since: Option<SystemTime>, now: SystemTime) -> String {
    let Some(s) = since else { return String::new() };
    let secs = now.duration_since(s).unwrap_or_default().as_secs_f64();
    match secs {
        s if s < 60. => "now".into(),
        s if s < 3600. => format!("{}m", (s / 60.).round()),
        s if s < 86_400. => format!("{}h", (s / 3600.).round()),
        s => format!("{}d", (s / 86_400.).round()),
    }
}

impl Bell {
    pub(crate) fn new(list: Attention, now: SystemTime, cx: &mut Context<Self>) -> Bell {
        Bell {
            list,
            now,
            on: 0,
            focus: cx.focus_handle(),
        }
    }

    fn live(&self) -> Vec<&Item> {
        self.list.items.iter().filter(|i| !i.fixed).collect()
    }

    /// The highlighted item.
    fn current(&self) -> Option<Item> {
        self.live().get(self.on).map(|i| (*i).clone())
    }

    /// A new list: the highlight stays on its item when it is still there.
    pub(crate) fn set(&mut self, list: Attention, now: SystemTime, cx: &mut Context<Self>) {
        let id = self.current().map(|i| i.id);
        self.list = list;
        self.now = now;
        let live = self.live();
        self.on = id
            .and_then(|id| live.iter().position(|i| i.id == id))
            .unwrap_or(self.on)
            .min(live.len().saturating_sub(1));
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.live().len();
        if n > 0 {
            self.on = self.on.saturating_add_signed(by).min(n - 1);
            cx.notify();
        }
    }

    fn first_fix(&mut self, cx: &mut Context<Self>) {
        if let Some(it) = self.current() {
            match it.fixes.first().cloned() {
                Some(f) => cx.emit(BellEvent::Fix(it, f)),
                None => cx.emit(BellEvent::Show(it)),
            }
        }
    }

    fn row(&self, t: &Tokens, it: &Item, on: bool, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let tone = if it.fixed {
            t.good
        } else if it.red {
            t.bad
        } else {
            t.warn
        };
        let mut text = Self::words(t, it);
        if !it.fixed {
            text = text.child(self.fixes(t, it, i, cx));
        }
        let right = if it.fixed {
            div().text_color(t.good).child("fixed ✓")
        } else {
            div()
                .text_color(t.dim)
                .child(SharedString::from(age(it.since, self.now)))
        };
        div()
            .id(("ni", i))
            .probe(
                format!("need-{}", it.id),
                format!("{} · {}", it.title, it.detail),
            )
            .flex()
            .items_start()
            .gap(px(10.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(t.line)
            .when(on, |d| d.bg(t.sel))
            .child(
                div()
                    .mt(px(6.))
                    .size(px(8.))
                    .flex_none()
                    .rounded_full()
                    .bg(tone),
            )
            .child(text)
            .child(right.flex_none().text_size(px(ty::XS_UP)).mt(px(1.)))
            .into_any_element()
    }

    /// An item's title, its stack and its detail.
    fn words(t: &Tokens, it: &Item) -> gpui::Div {
        let mut text = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .font_weight(ty::SEMIBOLD)
                    .text_color(if it.fixed { t.dim } else { t.bright })
                    .child(SharedString::from(it.title.clone()))
                    .when(!it.stack.is_empty(), |d| {
                        d.child(
                            div()
                                .font_weight(ty::REGULAR)
                                .text_color(t.dim)
                                .child(SharedString::from(format!("· {}", it.stack))),
                        )
                    }),
            );
        if !it.detail.is_empty() {
            let d = div()
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(SharedString::from(it.detail.clone()));
            text = text.child(if it.mono {
                d.font_family(MONO_FAMILY).text_size(px(ty::XS_UP))
            } else {
                d
            });
        }
        text
    }

    /// An item's buttons: its fixes, Show when no fix shows it, Not now.
    fn fixes(&self, t: &Tokens, it: &Item, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().flex().items_center().gap(px(6.)).mt(px(4.));
        for (k, f) in it.fixes.iter().enumerate() {
            let (item, fix) = (it.clone(), f.clone());
            let kind = if f.primary { Btn::Primary } else { Btn::Plain };
            let b = btn(t, ("fix", i * 16 + k), &f.label, "", kind, None)
                .h(px(24.))
                .px(px(10.))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(BellEvent::Fix(item.clone(), fix.clone()))
                }));
            row = row.child(b);
        }
        if it.show.is_some() && !it.fixes.iter().any(|f| f.kind == FixKind::Show) {
            let item = it.clone();
            row = row.child(
                btn(t, ("show", i), "Show", "", Btn::Quiet, None)
                    .h(px(24.))
                    .px(px(8.))
                    .on_click(
                        cx.listener(move |_, _, _, cx| cx.emit(BellEvent::Show(item.clone()))),
                    ),
            );
        }
        let item = it.clone();
        row.child(div().flex_1())
            .child(
                div()
                    .id(("nn", i))
                    .ml(px(4.))
                    .text_size(px(ty::XS_UP))
                    .text_color(t.dim)
                    .cursor_pointer()
                    .hover(|d| d.text_color(t.text))
                    .child("Not now")
                    .on_click(
                        cx.listener(move |_, _, _, cx| cx.emit(BellEvent::NotNow(item.clone()))),
                    ),
            )
            .into_any_element()
    }

    fn section(t: &Tokens, red: bool) -> AnyElement {
        let (title, why, c) = if red {
            (
                "Broken now",
                "· something you asked for is not happening",
                t.bad,
            )
        } else {
            ("Worth doing", "· nothing broken; waiting on you", t.warn)
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(16.))
            .pt(px(10.))
            .pb(px(6.))
            .border_b_1()
            .border_color(t.line)
            .child(tracked(title, ty::XS, ty::HEADING, c, 0.66))
            .child(div().text_size(px(ty::XS_UP)).text_color(t.dim).child(why))
            .into_any_element()
    }

    fn body(&self, t: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        if self.list.items.is_empty() {
            return super::bell_calm::calm(t, self.list.hidden);
        }
        let mut body = div()
            .id("nbody")
            .flex()
            .flex_col()
            .max_h(px(520.))
            .overflow_y_scroll();
        let live_on = self.current().map(|i| i.id);
        for red in [true, false] {
            let items: Vec<&Item> = self.list.items.iter().filter(|i| i.red == red).collect();
            if items.is_empty() {
                continue;
            }
            body = body.child(Self::section(t, red));
            for it in items {
                let on = live_on.as_deref() == Some(it.id.as_str()) && !it.fixed;
                let i = self
                    .list
                    .items
                    .iter()
                    .position(|x| x.id == it.id)
                    .unwrap_or(0);
                body = body.child(self.row(t, it, on, i, cx));
            }
        }
        body.into_any_element()
    }

    fn head(&self, t: &Tokens) -> impl IntoElement {
        let a = &self.list;
        let mut said = Vec::new();
        if a.red > 0 {
            said.push(format!("{} broken now", a.red));
        }
        if a.yellow > 0 {
            said.push(format!("{} worth doing", a.yellow));
        }
        div()
            .flex()
            .items_baseline()
            .gap(px(10.))
            .px(px(16.))
            .pt(px(14.))
            .pb(px(10.))
            .border_b_1()
            .border_color(t.line)
            .child(
                div()
                    .text_size(px(ty::MD_UP))
                    .font_weight(ty::HEADING)
                    .text_color(t.bright)
                    .child("Needs you"),
            )
            .child(
                div()
                    .text_size(px(ty::SM))
                    .text_color(t.dim)
                    .child(SharedString::from(said.join(" · "))),
            )
            .child(div().flex_1())
            .when(a.hidden > 0, |d| {
                d.child(
                    div()
                        .text_size(px(ty::SM))
                        .text_color(t.dim)
                        .child(SharedString::from(format!("{} put aside", a.hidden))),
                )
            })
    }

    fn foot(&self, t: &Tokens) -> impl IntoElement {
        let k = |key: &str, what: &str| {
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .mr(px(4.))
                        .child(kbd_chip(t, key, ty::MICRO, ty::SEMIBOLD, 5.)),
                )
                .child(SharedString::from(what.to_owned()))
        };
        let f = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .px(px(16.))
            .border_t_1()
            .border_color(t.line)
            .text_size(px(ty::XS_UP))
            .text_color(t.dim)
            .whitespace_nowrap();
        if self.list.items.is_empty() {
            return f
                .child("Items leave by themselves once fixed")
                .child(div().flex_1())
                .child(kbd_chip(t, "⌘⇧N", ty::MICRO, ty::SEMIBOLD, 5.));
        }
        f.child(k("↑↓", "move"))
            .child(k("⏎", "first fix"))
            .child(k("⌘⏎", "show"))
            .child(k("n", "not now"))
            .child(div().flex_1())
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child("leaves by itself once fixed"),
            )
    }
}

impl Render for Bell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let body = self.body(&t, cx);
        div()
            .id("bell-panel")
            .key_context("Bell")
            .track_focus(&self.focus)
            .w(px(480.))
            .font_family(UI_FAMILY)
            .text_size(px(ty::BASE))
            .line_height(px(ty::LH_19))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(t.line2)
            .bg(t.panel)
            .shadow(super::shadow(&t))
            .occlude()
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &BellFix, _, cx| this.first_fix(cx)))
            .on_action(cx.listener(|this, _: &BellShow, _, cx| {
                if let Some(it) = this.current() {
                    cx.emit(BellEvent::Show(it));
                }
            }))
            .on_action(cx.listener(|this, _: &BellNotNow, _, cx| {
                if let Some(it) = this.current() {
                    cx.emit(BellEvent::NotNow(it));
                }
            }))
            .child(self.head(&t))
            .child(body)
            .child(self.foot(&t))
    }
}
