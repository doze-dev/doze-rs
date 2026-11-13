//! The layer a dialog or a sheet sits in: the scrim over the whole window, the panel centred on it, both entering
//! together (the scrim fades, the panel fades and rises a few pixels: 180 ms, none when the system reduces motion). A
//! press on the scrim, outside the panel, cancels. The page stays drawn underneath.

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, BoxShadow, Hsla, IntoElement, MouseButton,
    SharedString, Window, div, point, prelude::*, px,
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
    let dim = if t.dark {
        alpha(crate::theme::mix(t.well, gpui::black(), 50.), 0.62)
    } else {
        t.scrim()
    };
    let scrim = full().bg(dim);
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
    let ease = Animation::new(motion.normal).with_easing(gpui::ease_out_quint());
    let travel = motion.travel;
    let scrim = scrim.with_animation(
        SharedString::from(format!("{id}-scrim")),
        ease.clone(),
        |d, p| d.opacity(p),
    );
    // The holder is the full window, so it rises by its top edge; the panel inside stays centred.
    let holder = holder.with_animation(
        SharedString::from(format!("{id}-panel")),
        ease,
        move |d, p| d.opacity(p).top(px(travel * (1.0 - p))),
    );
    full()
        .occlude()
        .child(scrim)
        .child(holder)
        .into_any_element()
}

/// `.dialog`: the panel a dialog or a sheet is drawn in, `width` across, its title at the top. It keeps a press inside
/// it from reaching the scrim.
pub(crate) fn panel(t: &Tokens, width: f32, gap: f32, title: String) -> gpui::Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .w(px(width))
        .flex()
        .flex_col()
        .gap(px(gap))
        .px(px(22.))
        .py(px(20.))
        .rounded(px(12.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .shadow(super::shadow(t))
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
