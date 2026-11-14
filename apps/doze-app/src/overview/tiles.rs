//! The Tiles view (the Tiles board): a tile per service under its group, four across (three, then two, as the window
//! narrows), each with its dot, name, pin and switch, its word and why, its memory and address. Also the row parts the
//! List shares: the dot, the switch, the word's tone, the filter box.

use doze_desk::overview::Row;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use gpui_component::input::Input;

use super::OverviewView;
use doze_display::Look;

use crate::instance::ui::state_mark;
use crate::kit::{chip_of, s, word_line};
use crate::outward::Probe as _;
use crate::theme::{Tokens, mix, ty};
use crate::widgets::{Mono as _, dot, icon, switch, tip, tracked};
use doze_desk::overview::titles;

/// Tiles to a row at the window's width.
pub(super) fn per_row(v: &OverviewView) -> usize {
    match v.width {
        w if w <= 860. => 2,
        w if w <= 1100. => 3,
        _ => 4,
    }
}

/// The row's mark: the same as the sidebar's and the instance head's (`ui::state_mark`), from the words' look. Go's rows
/// drew one dot for off, paused, failed and crashing alike where its sidebar drew ○, ⊘ and ✗; here every surface draws
/// the one vocabulary.
pub(super) fn row_dot(t: &Tokens, r: &Row) -> AnyElement {
    let trouble = r.trouble && r.kind != "task";
    match Look::of_word(r.look.word) {
        Some(l) => state_mark(t, Some(l), trouble, 8.),
        None => dot(if trouble { t.bad } else { t.dim }).into_any_element(),
    }
}

/// The word's tone (`wordTone`): the look's own, as every surface draws it; red in trouble; a verb in flight cool.
pub(super) fn word_tone(t: &Tokens, r: &Row) -> gpui::Hsla {
    if r.trouble {
        return t.bad;
    }
    match Look::of_word(r.look.word) {
        Some(l) => t.tone(l.tone()),
        None if r.look.word == "ready" => t.accent,
        // Any other word takes the line's own colour, as `wordTone` gave it no class.
        None => t.dim,
    }
}

/// The row's switch (`Switch`): Run for a task, ⊘ when paused in doze.hcl, else the switch; off for a down row.
/// It shows the state the daemon last said, never where a click is taking it (Go: no optimistic flip): a click sends
/// the verb, and the switch moves when the daemon's update arrives. `stack` keeps one stack's knobs apart from
/// another's.
pub(super) fn row_switch(
    t: &Tokens,
    r: &Row,
    stack: &str,
    cx: &mut Context<OverviewView>,
) -> AnyElement {
    let n = r.name.clone();
    // The switch is not a press on the tile: it neither selects it nor counts towards a double click (Go stops it).
    let say = cx.listener(move |this, _, _, cx| {
        cx.stop_propagation();
        this.toggle_row(&n, cx);
    });
    let id = gpui::SharedString::from(format!("sw-{}", r.name));
    if r.kind == "task" {
        if r.down {
            return div().into_any_element();
        }
        let label = if matches!(Look::of_word(r.look.word), Some(Look::Done | Look::Failed)) {
            "Run again"
        } else {
            "Run"
        };
        return div()
            .id(id)
            .flex_none()
            .h(px(24.))
            .px(px(10.))
            .rounded(px(6.))
            .border_1()
            .border_color(t.accent)
            .flex()
            .items_center()
            .text_size(px(ty::XS))
            .font_weight(ty::SEMIBOLD)
            .text_color(t.accent)
            .hover(|d| d.bg(t.sel))
            .child(label)
            .probe(format!("run-{}", r.name), label)
            .on_click(say)
            .into_any_element();
    }
    if r.paused && !r.down {
        // `.lock`: the glyph in a dashed pill the size of a switch; only the file brings it back.
        return div()
            .id(id)
            .tooltip(tip(titles::LOCK))
            .flex_none()
            .w(px(34.))
            .h(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(9.))
            .border_1()
            .border_dashed()
            .border_color(t.line2)
            .text_size(px(ty::BASE))
            .text_color(t.dim)
            .child(r.look.glyph)
            .into_any_element();
    }
    div()
        .id(id)
        .flex_none()
        .child(switch(
            t,
            &format!("{stack}/{}", r.name),
            r.on,
            false,
            r.off || r.frozen,
        ))
        .tooltip(tip(titles::switch(r)))
        .when(!r.frozen, |d| d.cursor_pointer().on_click(say))
        .probe(
            format!("sw-{}", r.name),
            format!("{} is {}", r.name, r.look.word),
        )
        .into_any_element()
}

/// `.toolbar .filter`: the filter box and its key. With the keys in it, its frame takes the accent's tint, easing
/// over 120 ms (Go `.filter:focus-within`, `transition: border-color 120ms`).
pub(super) fn filter(t: &Tokens, v: &OverviewView) -> impl IntoElement {
    let (frame, moving) = crate::widgets::input_frame(t, "overview/filter", v.filter_on);
    div().flex().flex_none().px(px(28.)).pb(px(14.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .w(px(260.))
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
}

/// A group's heading over its tiles (`.gl`): the title and what it is.
fn group_head(t: &Tokens, title: &str, what: &str) -> impl IntoElement {
    let title = format!(
        "{}{}",
        title.get(..1).unwrap_or(""),
        title.get(1..).unwrap_or("").to_lowercase()
    );
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .mx(px(28.))
        .mb(px(12.))
        .child(tracked(&title, ty::XS, ty::BOLD, t.dim, 1.1))
        .child(
            div()
                .text_size(px(ty::XS))
                .text_color(t.dim)
                .opacity(0.8)
                .child(s(what)),
        )
}

/// The tile's top line: dot, name, pin, switch.
fn tile_top(
    t: &Tokens,
    r: &Row,
    (dim, stack): (bool, &str),
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let name = div()
        .text_size(px(ty::MD_UP))
        .font_weight(ty::HEADING)
        .text_color(if dim { t.dim } else { t.bright })
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(s(&r.name));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .child(row_dot(t, r))
        .child(name)
        .when(r.pinned, |d| d.child(icon("icons/pin.svg", 13., t.accent)))
        // The lock stands where the switch would (the mark by the name already says ⊘).
        .child(div().flex_1())
        .child(row_switch(t, r, stack, cx))
}

/// The word, in its tone, and why.
fn tile_state(t: &Tokens, r: &Row) -> impl IntoElement {
    div()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_color(t.dim)
        .child(word_line(r.look.word, word_tone(t, r), &[&r.because]))
}

/// One tile.
fn tile(
    t: &Tokens,
    r: &Row,
    (selected, stack): (bool, &str),
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let off = !r.on && r.kind != "task";
    // As shell.css orders them: in trouble, then off (no fill, a dashed frame, even when selected), then selected. A
    // failed one is both: its red frame, dashed, and no fill (doze stopped trying; it waits for you).
    let (border, bg) = if r.trouble {
        let fill = if off {
            gpui::transparent_black()
        } else {
            mix(t.bad, t.panel, 7.)
        };
        (mix(t.bad, t.line, 55.), fill)
    } else if off {
        (
            if selected { t.line2 } else { t.line },
            gpui::transparent_black(),
        )
    } else if selected {
        (t.line2, mix(t.accent, t.panel, 7.))
    } else {
        (t.line, t.panel)
    };
    // Hover and selection ease over 120 ms (Go `.tile`'s transition); a red tile keeps its frame under the pointer.
    let hid = format!("tile/{stack}/{}", r.name);
    let border = if crate::motion::hovered(&hid) && !r.trouble {
        t.line2
    } else {
        border
    };
    let (border, b_moving) = crate::motion::colour(&format!("{hid}/border"), border);
    let (bg, g_moving) = crate::motion::colour(&format!("{hid}/bg"), bg);
    let name = r.name.clone();
    let el = div()
        .id(gpui::SharedString::from(format!("tile-{}", r.name)))
        .flex()
        .flex_col()
        .gap(px(8.))
        .flex_1()
        .min_w_0()
        .min_h(px(112.))
        .pt(px(16.))
        .px(px(18.))
        .pb(px(14.))
        .rounded(px(12.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .when(off, |d| d.border_dashed())
        .child(crate::motion::tick(b_moving || g_moving))
        .child(tile_top(t, r, (off && !selected, stack), cx))
        .child(tile_state(t, r))
        .child(super::foot::tile_foot(t, r, cx))
        .probe(format!("tile-{}", r.name), r.name.clone())
        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
            this.select(&name, cx);
            if ev.click_count() > 1 {
                this.open(&name, None, cx);
            }
        }));
    crate::motion::track(el, &hid)
}

/// The view: the filter, then each group's tiles.
pub(super) fn view(
    t: &Tokens,
    v: &OverviewView,
    cx: &mut Context<OverviewView>,
) -> impl IntoElement {
    let per = per_row(v);
    let shown: Vec<&str> = v.shown().iter().map(|r| r.name.as_str()).collect();
    let mut body = div()
        .id("tiles")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&v.tiles_scroll);
    if v.home.groups.is_empty() && v.home.stopped.is_none() {
        body = body.child(div().mx(px(28.)).text_color(t.dim).child(
            "doze.hcl declares nothing yet: add an app or a module to it, and it shows here.",
        ));
    }
    for g in &v.home.groups {
        let rows: Vec<&Row> = g
            .rows
            .iter()
            .filter(|r| shown.contains(&r.name.as_str()))
            .collect();
        if rows.is_empty() {
            continue;
        }
        body = body.child(group_head(t, g.title, g.what));
        let mut grid = div().flex().flex_col().gap(px(14.)).mx(px(28.)).mb(px(26.));
        for chunk in rows.chunks(per) {
            let mut line = div().flex().gap(px(14.));
            for r in chunk {
                line = line.child(tile(
                    t,
                    r,
                    (
                        v.selected.as_deref() == Some(r.name.as_str()),
                        v.home.key.as_str(),
                    ),
                    cx,
                ));
            }
            for _ in chunk.len()..per {
                // The empty places are framed as a tile is (padding and border), so every tile is the same width.
                line = line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(18.))
                        .border_1()
                        .border_color(gpui::transparent_black()),
                );
            }
            grid = grid.child(line);
        }
        body = body.child(grid);
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(filter(t, v))
        .child(body)
}
