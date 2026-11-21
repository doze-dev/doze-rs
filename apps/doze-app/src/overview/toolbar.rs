//! The toolbar over the Tiles, the List and the Grid: the filter box and its key, and Sleep all at the head of the
//! services' switches; and the stack head's ⋯ menu as the overview answers it (the Library answers it for its rows).

use doze_desk::act::StackAction;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use gpui_component::input::Input;

use super::OverviewView;
use crate::kit::{chip_of, s};
use crate::outward::Probe as _;
use crate::theme::{Tokens, ty};
use crate::widgets::{Mono as _, switch, tip};

/// `.toolbar .filter`: the filter box and its key. With the keys in it, its frame takes the accent's tint, easing
/// over 120 ms (Go `.filter:focus-within`, `transition: border-color 120ms`).
/// How far in from the toolbar's right end the services' switches end: a list row's padding (14), a tile's (18) and
/// its border (1). Sleep all ends there too, so it stands at the head of their column.
pub(super) const LIST_SWITCH_IN: f32 = 14.;
pub(super) const TILE_SWITCH_IN: f32 = 19.;

pub(super) fn filter(
    t: &Tokens,
    v: &OverviewView,
    switch_in: Option<f32>,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let (frame, moving) = crate::widgets::input_frame(t, "overview/filter", v.filter_on);
    let all = switch_in.and_then(|inset| sleep_all(t, v, inset, cx));
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(28.))
        .pb(px(14.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .w(px(260.))
                .min_w(px(140.))
                .flex_shrink(1.)
                .pl(px(2.))
                .pr(px(8.))
                .rounded(px(7.))
                .border_1()
                .border_color(frame)
                .bg(t.well)
                .child(crate::motion::tick(moving))
                .child(
                    div().flex_1().child(
                        Input::new(&v.filter)
                            .appearance(false)
                            .text_size(px(ty::SM)),
                    ),
                )
                // `.filter kbd`: a quiet chip, dim on the selection colour.
                .child(
                    div()
                        .px(px(5.))
                        .rounded(px(4.))
                        .bg(t.sel)
                        .mono_face()
                        .text_size(px(ty::XS))
                        .line_height(px(ty::LH_15))
                        .font_weight(ty::SEMIBOLD)
                        .text_color(t.dim)
                        .child(s(&chip_of("Filter"))),
                ),
        )
        .child(div().flex_1())
        .children(all)
}

/// Sleep all, at the head of the services' switches (the owner's place for it: "right above all the other toggles"):
/// the words, then a switch the size of theirs, lined up with them, so it reads as the switch for all of them. On
/// while anything is awake; off puts every service to sleep and the daemon stays up (`doze sleep`). It does not turn
/// back on: the next connection wakes what it needs, so all asleep says so and waits.
fn sleep_all(
    t: &Tokens,
    v: &OverviewView,
    inset: f32,
    cx: &mut Context<OverviewView>,
) -> Option<AnyElement> {
    use crate::library::menu::{MOON_TITLE, Moon};
    let state = v.moon(cx);
    if state == Moon::Hidden {
        return None;
    }
    let words = match state {
        Moon::Working => "putting all to sleep…",
        _ if v.home.asleep => doze_desk::ALL_ASLEEP,
        _ => "Sleep all",
    };
    let mark = if state == Moon::Working {
        crate::widgets::spinner("sleep-all-spin", 13., t.cool)
    } else {
        crate::widgets::icon("icons/moon.svg", 14., t.dim).into_any_element()
    };
    let ready = state == Moon::Ready;
    let el = div()
        .id("sleep-all")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .mr(px(inset))
        .text_size(px(ty::SM))
        .text_color(if v.home.asleep || state == Moon::Working {
            t.cool
        } else {
            t.dim
        })
        .child(mark)
        .child(s(words))
        .child(switch(
            t,
            &format!("{}/all", v.home.key),
            !v.home.asleep && state != Moon::Working,
            false,
            !ready,
        ))
        .tooltip(tip(MOON_TITLE))
        .when(ready, |d| {
            d.cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.sleep_all(cx)))
        })
        .probe("sleep-all", words);
    Some(el.into_any_element())
}

impl crate::library::menu::MenuHost for OverviewView {
    fn menu_act(&mut self, _: &str, action: StackAction, cx: &mut Context<Self>) {
        self.menu = false;
        self.stack_act(action, cx);
    }

    fn menu_sleep(&mut self, _: &str, cx: &mut Context<Self>) {
        self.menu = false;
        self.sleep_all(cx);
        cx.notify();
    }

    fn menu_choose(&mut self, _: &str, cx: &mut Context<Self>) {
        self.menu = false;
        self.choose_start(cx);
    }

    fn menu_reveal(&mut self, dir: &str, file: bool, cx: &mut Context<Self>) {
        self.menu = false;
        let dir = std::path::Path::new(dir);
        if file {
            crate::outward::open_path(cx, &dir.join("doze.hcl"));
        } else {
            crate::outward::reveal(cx, dir);
        }
        cx.notify();
    }

    fn menu_close(&mut self, cx: &mut Context<Self>) {
        self.menu = false;
        cx.notify();
    }
}
