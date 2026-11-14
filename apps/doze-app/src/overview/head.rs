//! The page's head (name, folder, the view switch, the stack's switch), its banners, the strip of figures (or, for a
//! stopped stack, the strip that says what Up starts) and the keys bar.

use gpui::{Context, IntoElement, div, prelude::*, px, relative};

use super::{OverviewView, View};
use crate::kit::{Btn, btn, density, s};
use crate::outward::Probe as _;
use crate::theme::{Tokens, ty};
use crate::widgets::{switch, tag, tip};
use doze_desk::overview::titles;

/// `.head`: the name, the folder and how long up (or stopped and when); the views; Activity, All logs; Up.
pub(super) fn head(
    t: &Tokens,
    v: &OverviewView,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let h = &v.home;
    let mut parts: Vec<String> = vec![h.dir.clone()];
    if !h.up.is_empty() {
        parts.push(format!("up {}", h.up));
    }
    if let Some(st) = &h.stopped {
        parts.extend([
            doze_desk::overview::Look::STOPPED.word.to_owned(),
            st.when.clone(),
        ]);
    }
    parts.retain(|p| !p.is_empty());
    let path = parts.join(" · ");
    let mut row = title_row(t, &h.stack, &path);
    if v.view == View::List {
        let e = cx.entity();
        row = row.child(density(t, v.density, move |d| {
            let e = e.clone();
            Box::new(move |_, _, cx| e.update(cx, |this, cx| this.set_density(d, cx)))
        }));
    }
    let busy = v.busy(cx).0;
    let row = row
        .relative()
        .child(views(t, v, cx))
        // Go's keys on these are A and L (shift-a, shift-l: `e.key` "A", "L"); ⌘L also opens All logs.
        .child(go(
            t,
            "Activity",
            "A",
            titles::ACTIVITY,
            &crate::actions::Activity,
        ))
        .child(go(
            t,
            "All logs",
            "L",
            titles::ALL_LOGS,
            &crate::actions::AllLogs,
        ))
        .child(up(
            t,
            (h.running, busy),
            (&h.key, v.menu),
            h.stopped.as_ref().is_some_and(|s| !s.problem.is_empty()),
            cx,
        ));
    // Go: the same `StackMenu` as the Library row's, for this stack.
    let st = doze_desk::library::LibStack {
        key: h.key.clone(),
        name: h.stack.clone(),
        dir: h.dir.clone(),
        running: h.running,
        live: true,
        declared: h.total,
        ..Default::default()
    };
    row.when(v.menu, |d| {
        d.child(crate::library::menu::menu(
            t,
            &st,
            (MENU_AT, busy.is_some()),
            cx,
        ))
    })
}

/// Where the head's menu hangs: 8 px into the head's foot, as Go's `top: calc(100% - 8px)`.
const MENU_AT: f32 = 70.;

/// The segmented switch: Tiles · List · Map · Grid (new).
fn views(t: &Tokens, v: &OverviewView, cx: &mut Context<OverviewView>) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .rounded(px(7.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .overflow_hidden()
        .children(View::ALL.into_iter().enumerate().map(|(i, view)| {
            let on = v.view == view;
            let label = match view {
                View::Tiles => "Tiles",
                View::List => "List",
                View::Map => "Map",
                View::Grid => "Grid",
            };
            div()
                .id(("view", i))
                .h(px(28.))
                .px(px(11.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(ty::SM))
                .font_weight(ty::MEDIUM)
                .cursor_pointer()
                .when(on, |d| d.bg(t.sel).text_color(t.bright))
                .when(!on, |d| d.text_color(t.dim).hover(|d| d.text_color(t.text)))
                .when(i < 3, |d| d.border_r_1().border_color(t.line))
                .child(label)
                .when(view == View::Grid, |d| d.child(tag(t, "new")))
                .probe(format!("view-{}", view.word()), label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx)))
        }))
}

/// `.swl`: Up and the stack's switch; ⋯.
/// While the stack is busy the label says what it is doing ("starting…") and the switch, faded, where it is going.
fn up(
    t: &Tokens,
    (running, busy): (bool, Option<doze_desk::act::Busy>),
    (key, menu): (&str, bool),
    broken: bool,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let label = busy.map_or_else(|| "Up".to_owned(), |b| format!("{}…", b.word()));
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(ty::SM))
                .font_weight(ty::SEMIBOLD)
                .text_color(if busy.is_some() { t.cool } else { t.bright })
                .child(label),
        )
        .child(
            div()
                .id("stack-switch")
                .cursor_pointer()
                .child(switch(
                    t,
                    &format!("stack/{key}"),
                    busy.map_or(running, doze_desk::act::Busy::heading_up),
                    true,
                    broken || busy.is_some(),
                ))
                .when(broken, |d| d.tooltip(tip(titles::BROKEN_UP)))
                .when(busy.is_none() && !broken, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.flip_stack(cx)))
                }),
        )
        .child(crate::library::menu::more(
            t,
            "stack-more",
            menu,
            |this: &mut OverviewView, _| this.menu = !this.menu,
            cx,
        ))
}

/// The head's left: the stack's name, and its folder and how long it is up.
fn title_row(t: &Tokens, stack: &str, path: &str) -> gpui::Div {
    let name = div()
        .min_w(px(72.))
        .max_w(relative(0.4))
        .flex_none()
        .text_size(px(ty::XL))
        .font_weight(ty::HEADING)
        .text_color(t.bright)
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(s(stack));
    let folder = div()
        .flex_shrink(1.)
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(28.))
        .pt(px(22.))
        .pb(px(18.))
        .min_h(px(58.))
        .child(name)
        .child(folder.text_color(t.dim).child(s(path)))
        .child(div().flex_1())
}

/// A quiet button that does what the action table's row does (Activity A, All logs L), with Go's chip and title.
fn go(
    t: &Tokens,
    label: &'static str,
    chip: &str,
    title: &'static str,
    a: &dyn gpui::Action,
) -> impl IntoElement {
    let a = a.boxed_clone();
    btn(
        t,
        label,
        label,
        chip,
        Btn::Quiet,
        Some(Box::new(move |_, window, cx| {
            window.dispatch_action(a.boxed_clone(), cx)
        })),
    )
    .tooltip(tip(title))
}
