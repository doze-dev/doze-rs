//! Overlays, never pages: ⌘K and the help sheet open over the page, which stays rendered under a scrim. Each enters
//! with a short fade and a few pixels of travel (the owner's bar: gentle, 120–280 ms), and simply appears when the
//! system reduces motion.

pub(crate) mod admin;
pub(crate) mod alerts;
pub(crate) mod bell;
mod bell_calm;
pub(crate) mod dialog;
mod glossary;
pub(crate) mod help;
pub(crate) mod palette;
mod palette_view;
pub(crate) mod preflight;
pub(crate) mod scrim;
pub(crate) mod start;
#[cfg(all(test, feature = "capture"))]
mod tests;
pub(crate) mod toasts;

use gpui::{
    AnimationExt as _, AnyElement, BoxShadow, IntoElement, SharedString, div, point, prelude::*, px,
};

use crate::theme::{Motion, Tokens, alpha};

/// What is open over the page.
pub(crate) enum Overlay {
    Palette(gpui::Entity<palette::Palette>),
    Help,
    Dialog(gpui::Entity<dialog::DialogView>),
    Preflight(gpui::Entity<preflight::PreflightView>),
    Start(gpui::Entity<start::StartView>),
    /// Needs you, under the bell.
    Bell(gpui::Entity<bell::Bell>),
    /// A privileged job: shown, then run with its output.
    Admin(gpui::Entity<admin::AdminView>),
}

/// The shadow an overlay casts: a tight one at its edge and a wide one under it, so a panel lifts off a light page as
/// clearly as off a dark one.
pub(crate) fn shadow(t: &Tokens) -> Vec<BoxShadow> {
    let (edge, wide) = if t.dark { (0.35, 0.5) } else { (0.12, 0.24) };
    vec![
        BoxShadow {
            color: alpha(gpui::black(), edge),
            offset: point(px(0.), px(2.)),
            blur_radius: px(6.),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: alpha(gpui::black(), wide),
            offset: point(px(0.), px(18.)),
            blur_radius: px(48.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

/// The scrim and the overlay over it, entering together: the scrim fades in, the panel fades and drops into place.
pub(crate) fn layer(
    t: &Tokens,
    panel: AnyElement,
    top: f32,
    motion: Motion,
    id: &'static str,
) -> AnyElement {
    let full = || div().absolute().top(px(0.)).left(px(0.)).size_full();
    // The palette sits on Go's lighter `.scrim.top`; the help sheet on the `.scrim` a dialog has.
    let scrim = full().bg(if id == "palette" {
        t.scrim()
    } else {
        t.dialog_scrim()
    });
    let holder = full()
        .flex()
        .justify_center()
        .items_start()
        .pt(px(top))
        .child(panel);
    if !motion.moves() {
        return full()
            .occlude()
            .child(scrim)
            .child(holder)
            .into_any_element();
    }
    // Go: the scrim's `fade` (160 ms ease-out); the panel's `grow` (180 ms, from 4 px above).
    let scrim = scrim.with_animation(
        SharedString::from(format!("{id}-scrim")),
        crate::motion::scrim_anim(),
        |d, p| d.opacity(p),
    );
    let holder = holder.with_animation(
        SharedString::from(format!("{id}-panel")),
        crate::motion::go_anim(180),
        move |d, p| d.opacity(p).top(px(-4.0 * (1.0 - p))),
    );
    full()
        .occlude()
        .child(scrim)
        .child(holder)
        .into_any_element()
}
