//! A stack's ⋯ menu (`StackMenu`, `.pop`): the switch beside it is Up as last time and Down; this holds the rest. It
//! opens over the page, never as a page. Each item acts at once (through `crate::act`), or asks first where Go asked.

use doze_desk::library::LibStack;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use doze_desk::act::StackAction;

use super::LibraryView;
use crate::kit::s;
use crate::overlays::shadow;
use crate::theme::Tokens;
use crate::widgets::{MONO_FAMILY, kbd_plain, w};

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

    fn el(
        self,
        t: &Tokens,
        i: usize,
        (dir, key): (&str, &str),
        cx: &mut Context<LibraryView>,
    ) -> AnyElement {
        let sub = div()
            .text_size(px(11.))
            .text_color(t.dim)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .when(self.what == Do::Reveal, |d| d.font_family(MONO_FAMILY))
            .child(s(&self.sub));
        let title = div()
            .font_weight(w(600.))
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
            .text_size(px(12.5))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .line_height(px(15.))
                    .child(title)
                    .child(sub),
            )
            .when(!self.key.is_empty(), |d| {
                d.child(kbd_plain(self.key, 11., t.dim, 500.))
            });
        if !self.on {
            return el.opacity(0.55).into_any_element();
        }
        let (what, dir, key) = (self.what, dir.to_owned(), key.to_owned());
        el.cursor_pointer()
            .hover(|d| d.bg(t.pick))
            .on_click(cx.listener(move |this, _, _, cx| match what {
                Do::Restart => this.act(&key, StackAction::Restart, cx),
                Do::Everything => this.act(&key, StackAction::UpAll, cx),
                Do::Choose => this.choose_start(&key, cx),
                Do::Remove => this.act(&key, StackAction::Remove, cx),
                Do::Edit => this.reveal(&dir, true, cx),
                Do::Reveal => this.reveal(&dir, false, cx),
            }))
            .into_any_element()
    }
}

/// The items, with `None` where a line separates them.
fn items(st: &LibStack, dir: &str) -> Vec<Option<Item>> {
    let gone = !st.missing.is_empty();
    let mut out = Vec::new();
    if st.running {
        let it = Item::new(
            format!("Restart {}", st.name),
            "stops everything and starts what was running",
            Do::Restart,
        );
        out.push(Some(it));
    }
    let every = if st.running {
        "Start everything else"
    } else {
        "Start everything"
    };
    let sub = format!("all {} declared, not only last time's", st.declared);
    out.push(Some(Item {
        on: !gone,
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
            on: !gone,
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
        ..Item::new("Remove from doze…", what, Do::Remove)
    }));
    out
}

/// The menu for `st`, under its ⋯.
pub(super) fn menu(t: &Tokens, st: &LibStack, cx: &mut Context<LibraryView>) -> impl IntoElement {
    let dir = expand(&st.dir);
    let els: Vec<AnyElement> = items(st, &dir)
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
    div()
        .absolute()
        .top(px(52.))
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
        .children(els)
}

/// A folder written with `~` again in full.
fn expand(dir: &str) -> String {
    match (dir.strip_prefix('~'), doze_display::home()) {
        (Some(rest), Some(h)) => format!("{h}{rest}"),
        _ => dir.to_owned(),
    }
}
