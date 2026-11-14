//! A stack's ⋯ menu (`StackMenu`, `.pop`): the switch beside it is Up as last time and Down; this holds the rest. It
//! opens over the page, never as a page. Each item acts at once (through `crate::act`), or asks first where Go asked.
//! The Library's rows and a stack page's head both open it (Go: the same `StackMenu`). Near the window's bottom it
//! opens upward, so all of it shows; a press outside it, or esc, closes it.

use std::cell::Cell;

use doze_desk::library::LibStack;
use gpui::AnimationExt as _;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use doze_desk::act::StackAction;

use crate::kit::s;
use crate::overlays::shadow;
use crate::theme::{Tokens, ty};
use crate::widgets::{Mono as _, icon, kbd_plain};

/// What a view that opens the menu does with its items.
pub(crate) trait MenuHost: 'static + Sized {
    /// Acts on the stack keyed `key` (and closes the menu).
    fn menu_act(&mut self, key: &str, action: StackAction, cx: &mut Context<Self>);
    /// The start sheet for the stopped stack keyed `key`.
    fn menu_choose(&mut self, key: &str, cx: &mut Context<Self>);
    /// Opens the doze.hcl in `dir` (`file`), or shows `dir` in Finder.
    fn menu_reveal(&mut self, dir: &str, file: bool, cx: &mut Context<Self>);
    /// Closes the menu.
    fn menu_close(&mut self, cx: &mut Context<Self>);
}

thread_local! {
    /// The open menu was measured to run past the window's bottom: it opens upward (Go: `up`, set once on mount).
    static UP: Cell<bool> = const { Cell::new(false) };
    /// Where the press that closed a menu from outside was: a press on its own ⋯ then closes it, not reopens it.
    static CLOSED_AT: Cell<Option<gpui::Point<gpui::Pixels>>> = const { Cell::new(None) };
    /// The press on a ⋯ was the one that closed its menu.
    static SWALLOW: Cell<bool> = const { Cell::new(false) };
}

/// A menu opens: measured again, downward first.
pub(crate) fn opened() {
    UP.set(false);
}

/// The ⋯ beside a switch: toggles the menu (`toggle`), and is lit while it is open.
pub(crate) fn more<V: MenuHost>(
    t: &Tokens,
    id: impl Into<gpui::ElementId>,
    open: bool,
    toggle: impl Fn(&mut V, &mut Context<V>) + 'static,
    cx: &mut Context<V>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(28.))
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(open, |d| d.bg(t.sel))
        .hover(|d| d.bg(t.sel))
        .child(icon(
            "icons/dots.svg",
            16.,
            if open { t.accent } else { t.dim },
        ))
        .on_mouse_down(gpui::MouseButton::Left, move |e, _, _| {
            // Only this ⋯'s own menu, closed by this very press, stays closed; another row's ⋯ opens its menu.
            SWALLOW.set(CLOSED_AT.take() == Some(e.position) && open);
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            if SWALLOW.take() {
                return;
            }
            opened();
            toggle(this, cx);
            cx.notify();
        }))
}

/// What an item does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Do {
    Restart,
    Everything,
    Choose,
    Remove,
    Edit,
    Reveal,
}

/// One item: what it says, its key, whether it can be chosen now.
struct Item {
    title: String,
    sub: String,
    key: &'static str,
    on: bool,
    what: Do,
}

impl Item {
    fn new(title: impl Into<String>, sub: impl Into<String>, what: Do) -> Item {
        Item {
            title: title.into(),
            sub: sub.into(),
            key: "",
            on: true,
            what,
        }
    }

    fn el<V: MenuHost>(
        self,
        t: &Tokens,
        i: usize,
        (dir, key): (&str, &str),
        cx: &mut Context<V>,
    ) -> AnyElement {
        let sub = div()
            .text_size(px(ty::XS))
            .text_color(t.dim)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .when(self.what == Do::Reveal, |d| d.mono_face())
            .child(s(&self.sub));
        let title = div()
            .font_weight(ty::SEMIBOLD)
            .text_color(if self.on { t.bright } else { t.dim })
            .child(s(&self.title));
        let el = div()
            .id(("stack-menu", i))
            .flex()
            .items_center()
            .gap(px(10.))
            .min_h(px(34.))
            .px(px(10.))
            .py(px(4.))
            .rounded(px(7.))
            .text_size(px(ty::SM_UP))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .line_height(px(ty::LH_15))
                    .child(title)
                    .child(sub),
            )
            .when(!self.key.is_empty(), |d| {
                d.child(kbd_plain(self.key, ty::XS, t.dim, ty::MEDIUM))
            });
        if !self.on {
            return el.opacity(0.55).into_any_element();
        }
        let (what, dir, key) = (self.what, dir.to_owned(), key.to_owned());
        el.cursor_pointer()
            .hover(|d| d.bg(t.pick))
            .on_click(cx.listener(move |this, _, _, cx| match what {
                Do::Restart => this.menu_act(&key, StackAction::Restart, cx),
                Do::Everything => this.menu_act(&key, StackAction::UpAll, cx),
                Do::Choose => this.menu_choose(&key, cx),
                Do::Remove => this.menu_act(&key, StackAction::Remove, cx),
                Do::Edit => this.menu_reveal(&dir, true, cx),
                Do::Reveal => this.menu_reveal(&dir, false, cx),
            }))
            .into_any_element()
    }
}

/// The items, with `None` where a line separates them.
fn items(st: &LibStack, dir: &str, busy: bool) -> Vec<Option<Item>> {
    let gone = !st.missing.is_empty();
    let mut out = Vec::new();
    if st.running {
        let it = Item::new(
            format!("Restart {}", st.name),
            "stops everything and starts what was running",
            Do::Restart,
        );
        out.push(Some(Item { on: !busy, ..it }));
    }
    let every = if st.running {
        "Start everything else"
    } else {
        "Start everything"
    };
    let sub = format!("all {} declared, not only last time's", st.declared);
    out.push(Some(Item {
        on: !gone && !busy,
        ..Item::new(every, sub, Do::Everything)
    }));
    if !st.running {
        let it = Item::new(
            "Choose what starts…",
            "tick the apps; what they need comes too",
            Do::Choose,
        );
        out.push(Some(Item {
            key: "c",
            on: !gone && !busy,
            ..it
        }));
    }
    out.push(None);
    out.push(Some(Item {
        on: !dir.is_empty() && !gone,
        ..Item::new("Edit doze.hcl", "in your editor", Do::Edit)
    }));
    let folder_there = !dir.is_empty() && st.missing != "the folder is gone";
    out.push(Some(Item {
        on: folder_there,
        ..Item::new("Show in Finder", st.dir.clone(), Do::Reveal)
    }));
    out.push(None);
    let what = if st.running {
        "stops it and stops listing it; every file stays"
    } else {
        "stops listing it; every file stays"
    };
    out.push(Some(Item {
        key: "⌫",
        on: !busy,
        ..Item::new("Remove from doze…", what, Do::Remove)
    }));
    out
}

/// The menu for `st`, under its ⋯ (`at` px below the top of the box it is in), or over it near the window's bottom
/// (its foot `at` px above that box's foot). `busy`: the stack is acting, so only what does not act is offered.
pub(crate) fn menu<V: MenuHost>(
    t: &Tokens,
    st: &LibStack,
    (at, busy): (f32, bool),
    cx: &mut Context<V>,
) -> impl IntoElement {
    let dir = expand(&st.dir);
    let els: Vec<AnyElement> = items(st, &dir, busy)
        .into_iter()
        .enumerate()
        .map(|(i, it)| match it {
            Some(it) => it.el(t, i, (&dir, &st.key), cx),
            None => div()
                .h(px(1.))
                .mx(px(6.))
                .my(px(3.))
                .bg(t.line)
                .into_any_element(),
        })
        .collect();
    let up = UP.get();
    let el = div()
        .id("stack-menu")
        .absolute()
        .when(up, |d| d.bottom(px(at)))
        .when(!up, |d| d.top(px(at)))
        .right(px(16.))
        .w(px(300.))
        .p(px(6.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .rounded(px(10.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .shadow(shadow(t))
        .occlude()
        .on_mouse_down_out(cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
            CLOSED_AT.set(Some(e.position));
            this.menu_close(cx);
        }))
        .child(measure(cx))
        .children(els);
    // Painted after the page, so the rows and banners below never draw over it.
    if crate::motion::reduced() {
        return gpui::deferred(el).with_priority(1).into_any_element();
    }
    // Go `.pop`: `settle`, 160 ms from 6 px above.
    let el = el.with_animation(
        if up { "menu-in-up" } else { "menu-in" },
        crate::motion::go_anim(160),
        move |d, p| {
            let d = d.opacity(p);
            if up {
                d.bottom(px(at + 6. * (1.0 - p)))
            } else {
                d.top(px(at - 6. * (1.0 - p)))
            }
        },
    );
    gpui::deferred(el).with_priority(1).into_any_element()
}

/// Measures where the menu fell, as Go's layout effect does: past the window's bottom, it opens upward from the next
/// frame.
fn measure<V: MenuHost>(cx: &mut Context<V>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    gpui::canvas(
        move |b, window, _| {
            let bottom = window.viewport_size().height - px(8.);
            if !UP.get() && b.bottom() > bottom {
                UP.set(true);
                let view = view.clone();
                window.on_next_frame(move |_, cx| {
                    let _ = view.update(cx, |_, cx| cx.notify());
                });
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_full()
}

/// A folder written with `~` again in full.
fn expand(dir: &str) -> String {
    match (dir.strip_prefix('~'), doze_display::home()) {
        (Some(rest), Some(h)) => format!("{h}{rest}"),
        _ => dir.to_owned(),
    }
}
