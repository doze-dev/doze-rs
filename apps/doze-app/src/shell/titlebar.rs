//! The client-drawn title bar: Home and a tab per stack (its dot, its name, × on a stopped one), the bell, help and
//! ⌘K, with room for the traffic lights.

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::{Root, Screen, Tab};
use crate::actions::{OpenBell, OpenPalette, ShowHelp};
use crate::kit::{chip_of, s};
use crate::outward::Probe as _;
use crate::theme::{Tokens, ty};
use crate::widgets::{dot, icon, kbd_plain, tip};

thread_local! {
    /// A press on the bar itself, not yet moved: the next move drags the window.
    static MOVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn tab(
    t: &Tokens,
    on: bool,
    id: impl Into<gpui::ElementId>,
    body: Vec<AnyElement>,
) -> gpui::Stateful<gpui::Div> {
    // `.stab`: the words (and the open tab's frame) ease over 120 ms as the pointer comes and goes.
    let id: gpui::ElementId = id.into();
    let hid = format!("stab/{id}");
    let over = crate::motion::hovered(&hid);
    let (fg, f_moving) = crate::motion::colour(
        &format!("{hid}/fg"),
        if on {
            t.bright
        } else if over {
            t.text
        } else {
            t.dim
        },
    );
    let (bg, g_moving) = crate::motion::colour(
        &format!("{hid}/bg"),
        if on { t.bg } else { gpui::transparent_black() },
    );
    let (frame, r_moving) = crate::motion::colour(
        &format!("{hid}/border"),
        if on {
            t.line
        } else {
            gpui::transparent_black()
        },
    );
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(31.))
        .px(px(14.))
        .mb(px(-1.))
        .max_w(px(180.))
        .rounded_t(px(8.))
        .border_1()
        .border_b_0()
        .text_size(px(ty::SM_UP))
        .whitespace_nowrap()
        .cursor_pointer()
        .bg(bg)
        .border_color(frame)
        .text_color(fg)
        .when(on, |d| d.font_weight(ty::SEMIBOLD))
        .child(crate::motion::tick(f_moving || g_moving || r_moving))
        .children(body);
    crate::motion::track(base, &hid)
}

/// `.bell` (the bell, and ? beside it): its mark turns bright under the pointer, easing over 120 ms ([`ink`]).
fn square(t: &Tokens, id: &'static str) -> gpui::Stateful<gpui::Div> {
    let el = div()
        .id(id)
        .mb(px(6.))
        .flex_none()
        .w(px(32.))
        .h(px(30.))
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer();
    crate::motion::track(el, &format!("bell/{id}"))
}

/// The mark of the square `id`: `rest`, or bright under the pointer, easing; and whether it is still moving.
fn ink(t: &Tokens, id: &str, rest: gpui::Hsla) -> (gpui::Hsla, bool) {
    let hid = format!("bell/{id}");
    let to = if crate::motion::hovered(&hid) {
        t.bright
    } else {
        rest
    };
    crate::motion::colour(&format!("{hid}/fg"), to)
}

/// A red count: the bell's, a tab's.
fn badge(t: &Tokens, n: usize) -> gpui::Div {
    div()
        .flex_none()
        .min_w(px(16.))
        .h(px(16.))
        .px(px(4.))
        .rounded_full()
        .bg(t.bad)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(ty::MICRO))
        .line_height(px(ty::LH_16))
        .font_weight(ty::BOLD)
        .text_color(gpui::white())
        .child(gpui::SharedString::from(n.to_string()))
}

/// The title bar.
pub(crate) fn titlebar(t: &Tokens, root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let (red, yellow) = (root.attn.list.red, root.attn.list.yellow);
    let (bell_ink, bell_moving) = ink(t, "bell", if red > 0 { t.bright } else { t.dim });
    let (help_ink, help_moving) = ink(t, "help", t.dim);
    let bell = square(t, "bell")
        .relative()
        .child(crate::motion::tick(bell_moving || help_moving))
        .probe("bell", format!("bell red={red} yellow={yellow}"))
        .when(root.bell_open(), |d| d.bg(t.sel))
        // Something broken: Go's "hot" bell, its border the bad colour 55% into the panel and no fill; open, it takes
        // the accent (the count still says).
        .when(red > 0 && !root.bell_open(), |d| {
            d.border_color(crate::theme::mix(t.bad, t.panel, 55.))
        })
        .when(root.bell_open(), |d| d.border_color(t.accent))
        .child(icon("icons/bell.svg", 16., bell_ink))
        .when(red == 0 && yellow > 0, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(3.))
                    .right(px(3.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(t.warn)
                    // Go `.bell .yd`: a 1.5 px ring of the panel round the dot (inside its 8 px).
                    .border(px(1.5))
                    .border_color(t.panel),
            )
        })
        .occlude()
        .tooltip(tip(format!("{}  ⌘⇧N", root.attn.list.bell_label())))
        .on_click(cx.listener(|this, _, window, cx| this.on_bell(&OpenBell, window, cx)));
    let help = square(t, "help")
        .text_size(px(ty::BASE))
        .font_weight(ty::SEMIBOLD)
        .text_color(help_ink)
        .child("?")
        .occlude()
        .tooltip(tip("Keyboard shortcuts  ?  ⌘/"))
        .on_click(cx.listener(|this, _, window, cx| this.on_help(&ShowHelp, window, cx)));
    // Double-click zooms the window, as any title bar does; the strip itself is the drag region (Go titlebar.tsx).
    // Tabs and buttons occlude it, so a press on them never moves the window.
    div()
        .id("titlebar")
        .on_click(|e, window, _| {
            if e.click_count() == 2 {
                window.zoom_window();
            }
        })
        .on_mouse_down(gpui::MouseButton::Left, |_, _, _| MOVE.set(true))
        .on_mouse_up(gpui::MouseButton::Left, |_, _, _| MOVE.set(false))
        .on_mouse_move(|_, window, _| {
            if MOVE.replace(false) {
                window.start_window_move();
            }
        })
        .flex()
        .flex_none()
        .items_end()
        .gap(px(10.))
        .h(px(42.))
        .pl(px(86.))
        .pr(px(14.))
        .bg(t.side)
        .border_b_1()
        .border_color(t.line)
        .child(tabs(t, root, cx))
        // The count sits over the bell's corner, outside its square (a child would be clipped to it).
        .child(div().relative().flex_none().child(bell).when(red > 0, |d| {
            d.child(badge(t, red).absolute().top(px(-4.)).right(px(-7.)))
        }))
        .child(help)
        .child(search(t, cx))
}

/// The ⌘K button.
fn search(t: &Tokens, cx: &mut Context<Root>) -> impl IntoElement {
    div()
        .id("search")
        .mb(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(30.))
        .px(px(12.))
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_size(px(ty::SM))
        .font_weight(ty::MEDIUM)
        .text_color(t.text)
        .cursor_pointer()
        .hover(|s| s.bg(t.sel))
        .occlude()
        .tooltip(tip("Search a name, or type a verb and a name (⌘K)"))
        .on_click(cx.listener(|this, _, window, cx| this.on_palette(&OpenPalette, window, cx)))
        .child(kbd_plain(
            &chip_of("Search or do…"),
            ty::XS,
            t.dim,
            ty::MEDIUM,
        ))
        .child("Search or do")
}

/// Home and each stack's tab.
fn tabs(t: &Tokens, root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let home_on = root.screen == Screen::Library;
    let home = tab(
        t,
        home_on,
        "tab-home",
        vec![
            icon(
                "icons/home.svg",
                15.,
                if home_on { t.bright } else { t.dim },
            )
            .into_any_element(),
            div().child("Home").into_any_element(),
        ],
    )
    .occlude()
    .tooltip(tip("Home  ⌘0"))
    .on_click(cx.listener(|this, _, window, cx| this.go(Screen::Library, window, cx)));
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_end()
        .gap(px(2.))
        .child(home)
        .children(
            root.tabs
                .iter()
                .enumerate()
                .map(|(i, tb)| stack_tab(t, root, i, tb, cx)),
        )
        .children(pending_tab(t, root, cx))
}

/// A folder with no doze.hcl yet, while its page shows: its own tab, as Go's pending tab; × leaves it for Home.
fn pending_tab(t: &Tokens, root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    let Screen::Empty(dir) = &root.screen else {
        return None;
    };
    let name = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let close = div()
        .id("tab-pending-close")
        .size(px(16.))
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .hover(|d| d.bg(t.line))
        .child(icon("icons/close.svg", 10., t.dim))
        .on_click(cx.listener(|this, _, window, cx| {
            cx.stop_propagation();
            this.go(Screen::Library, window, cx);
        }));
    let body = vec![
        dot(t.dim).into_any_element(),
        div()
            .overflow_hidden()
            .text_ellipsis()
            .child(name)
            .into_any_element(),
        close.into_any_element(),
    ];
    let tip_dir = dir.display().to_string();
    Some(
        tab(t, true, "tab-pending", body)
            .occlude()
            .tooltip(tip(tip_dir))
            .into_any_element(),
    )
}

fn stack_tab(t: &Tokens, root: &Root, i: usize, tb: &Tab, cx: &mut Context<Root>) -> AnyElement {
    let on = root.key() == Some(&tb.key);
    let tone = if tb.troubled > 0 {
        t.bad
    } else if tb.running && tb.live {
        t.good
    } else if tb.running {
        t.warn
    } else {
        t.dim
    };
    let mut body = vec![
        dot(tone).into_any_element(),
        div()
            .overflow_hidden()
            .text_ellipsis()
            .child(s(&tb.name))
            .into_any_element(),
    ];
    // As Go's tab: how many of its instances are in trouble, tasks aside. The bell counts Needs you's items.
    if tb.troubled > 0 {
        body.push(badge(t, tb.troubled).into_any_element());
    }
    // `.stabx`: on every tab, seen (opacity) while the tab is the one shown or under the pointer.
    let id: gpui::ElementId = ("tab", i).into();
    let over = crate::motion::hovered(&format!("stab/{id}"));
    {
        let k = tb.key.clone();
        body.push(
            div()
                .id(("tab-close", i))
                .opacity(if on || over { 1. } else { 0. })
                .tooltip(tip(if tb.running {
                    "Close the tab. The stack keeps running; open it again from Home."
                } else {
                    "Close the tab"
                }))
                .size(px(16.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.dim)
                .hover(|d| d.bg(t.line).text_color(t.bright))
                .child(icon("icons/close.svg", 10., t.dim))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.close_tab(&k, window, cx);
                }))
                .into_any_element(),
        );
    }
    let key = tb.key.clone();
    let title = format!(
        "{}{}  ⌘{}",
        tb.dir,
        if tb.running && !tb.live {
            " · daemon not answering"
        } else {
            ""
        },
        i + 1
    );
    tab(t, on, id, body)
        .occlude()
        .tooltip(tip(title))
        .on_click(
            cx.listener(move |this, _, window, cx| this.go(Screen::Stack(key.clone()), window, cx)),
        )
        .into_any_element()
}
