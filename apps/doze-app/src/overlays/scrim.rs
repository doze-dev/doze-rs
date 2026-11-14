//! The layer a dialog or a sheet sits in: the scrim over the whole window, the panel centred on it, both entering
//! together (the scrim fades, the panel fades and rises a few pixels: 180 ms, none when the system reduces motion). A
//! press on the scrim, outside the panel, cancels. The page stays drawn underneath.

use gpui::{
    AnimationExt as _, AnyElement, App, BoxShadow, Hsla, IntoElement, MouseButton, SharedString,
    Window, div, point, prelude::*, px,
};

use crate::theme::{Motion, Tokens, alpha, ty};

/// What a press on the scrim does.
pub(crate) type OnScrim = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// The highlighted button's ring (a focus ring, as the browser drew Go's).
pub(crate) fn ring(c: Hsla) -> BoxShadow {
    BoxShadow {
        color: alpha(c, 0.45),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(3.),
        inset: false,
    }
}

/// `panel` centred over the scrim.
pub(crate) fn center(
    t: &Tokens,
    panel: AnyElement,
    motion: Motion,
    id: &'static str,
    on_scrim: OnScrim,
) -> AnyElement {
    let full = || div().absolute().top(px(0.)).left(px(0.)).size_full();
    // A dialog dims the page more than the palette does (Go `.scrim`: .62 against `.scrim.top`'s .35).
    let scrim = full().bg(t.dialog_scrim());
    // The holder covers the window over the scrim, so it hears a press outside the panel (the panel stops its own).
    let holder = full()
        .flex()
        .justify_center()
        .items_center()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_scrim(window, cx))
        .child(panel);
    if !motion.moves() {
        return full()
            .occlude()
            .child(scrim)
            .child(holder)
            .into_any_element();
    }
    // Go: the scrim's `fade` (160 ms ease-out); the dialog's `grow` (180 ms, from 4 px above, its frame from .985 of
    // its size: [`panel`]). The holder is the full window, so it moves by its top edge; the panel inside stays centred.
    let scrim = scrim.with_animation(
        SharedString::from(format!("{id}-scrim")),
        crate::motion::scrim_anim(),
        |d, p| d.opacity(crate::motion::traced("dialog-scrim", p)),
    );
    let holder = holder.with_animation(
        SharedString::from(format!("{id}-panel")),
        crate::motion::go_anim(180),
        move |d, p| {
            let p = crate::motion::traced("dialog-panel", p);
            d.opacity(p).top(px(-4.0 * (1.0 - p)))
        },
    );
    full()
        .occlude()
        .child(scrim)
        .child(holder)
        .into_any_element()
}

/// A dialog's frame (fill, `border`, shadow), drawn behind its words: it grows from .985 of the dialog's size as it
/// enters (Go `grow`), the words staying as they are (GPUI scales no element).
pub(crate) fn frame(t: &Tokens, border: Hsla) -> AnyElement {
    grown_frame(t, border, 12., "dialog-frame")
}

/// A panel's frame (fill, `border`, shadow, `radius`) that grows from .985 of the panel's size as it enters (Go
/// `grow`: the dialog, the palette, the keys sheet), animated as `id`.
pub(crate) fn grown_frame(t: &Tokens, border: Hsla, radius: f32, id: &'static str) -> AnyElement {
    let frame = div()
        .absolute()
        .inset_0()
        .rounded(px(radius))
        .border_1()
        .border_color(border)
        .bg(t.panel)
        .shadow(super::shadow(t));
    if crate::motion::reduced() {
        return frame.into_any_element();
    }
    frame
        .with_animation(id, crate::motion::go_anim(180), move |d, p| {
            let k = gpui::relative(0.0075 * (1.0 - crate::motion::traced(id, p)));
            d.top(k).bottom(k).left(k).right(k)
        })
        .into_any_element()
}

/// `.dialog`: the panel a dialog or a sheet is drawn in, `width` across, its title at the top. It keeps a press inside
/// it from reaching the scrim.
///
/// Its frame (the fill, the border and the shadow) is drawn behind the words and grows from .985 of its size as it
/// enters, as Go's `grow` scales the dialog; GPUI scales no element, so the words stay as they are while the frame
/// grows around them.
pub(crate) fn panel(t: &Tokens, width: f32, gap: f32, title: String) -> gpui::Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .relative()
        .w(px(width))
        .flex()
        .flex_col()
        .gap(px(gap))
        .px(px(22.))
        .py(px(20.))
        .child(frame(t, t.line2))
        .font_family(crate::widgets::UI_FAMILY)
        .text_size(px(ty::BASE))
        .line_height(px(ty::LH_19))
        .child(
            div()
                .text_size(px(ty::LG))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(title),
        )
}
