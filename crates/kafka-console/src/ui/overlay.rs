//! Overlays: a drawer (a side panel) or a dialog (a centred modal) drawn OVER its parent page. The recipe is in
//! `docs/port/console/overlays.md`; the short form:
//!
//! - An overlay is a board of kind `drawer` or `dialog`. It has its own address, and a parent page: the page the board
//!   draws behind it.
//! - Its page function returns the overlay element: the panel's content (a heading with an id, the content, the buttons)
//!   wrapped by [`frame`]: a scrim that is a plain link to the parent, and the panel (`role=dialog`, `aria-modal`,
//!   `aria-labelledby`). That one element is what a script is sent (a fragment), and what a direct visit has in the
//!   document, after the parent, so the address is shareable and works with no script.
//! - [`over`] pairs a parent's `main` with that element; `ui::shell::document_over` puts the two in one document, the
//!   parent marked `inert` and scroll-locked by CSS alone.
//! - A link that opens an overlay in place is [`client::dialog_link`](super::client::dialog_link). A control that closes
//!   it is a link to the parent with `data-dialog-close` ([`close`]).

use maud::{Markup, html};

/// The two shapes of overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A panel on the right, as tall as the page under the top bar: one thing looked at in full.
    Drawer,
    /// A box in the middle: a question, or a form, that needs an answer.
    Dialog,
}

impl Kind {
    const fn class(self) -> &'static str {
        match self {
            Self::Drawer => "ovl ovl-drawer root",
            Self::Dialog => "ovl ovl-dialog root",
        }
    }

    const fn panel(self) -> &'static str {
        match self {
            Self::Drawer => "drawer-panel",
            Self::Dialog => "modal",
        }
    }
}

/// A page with an overlay open on it: the parent's content and the overlay element, which the document places apart
/// (the parent is inert, the overlay is not).
#[derive(Debug)]
pub struct Over {
    pub main: Markup,
    pub overlay: Markup,
}

/// The overlay element: a scrim (a link to `close_href`, so that closing needs no script) and the panel holding `body`.
///
/// `label_id` is the id of the heading inside `body` that names the panel.
#[must_use]
pub fn frame(kind: Kind, label_id: &str, close_href: &str, body: Markup) -> Markup {
    html! {
        div class=(kind.class()) data-overlay data-close=(close_href) {
            a class="scrim" href=(close_href) tabindex="-1" aria-hidden="true" data-overlay-close {}
            div class=(kind.panel()) role="dialog" aria-modal="true" aria-labelledby=(label_id) tabindex="-1" { (body) }
        }
    }
}

/// A parent's `main` with an overlay (made by [`frame`], usually by the overlay's own page function) over it.
#[must_use]
pub const fn over(parent_main: Markup, overlay: Markup) -> Over {
    Over {
        main: parent_main,
        overlay,
    }
}

/// The control that closes an overlay: a link to the parent (so it works with no script) that a script turns into a close.
#[must_use]
pub fn close(class: &str, href: &str, label: Markup) -> Markup {
    html! { a class=(class) href=(href) data-dialog-close { (label) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_a_scrim_that_links_to_the_parent_and_a_named_modal_panel() {
        let m = frame(
            Kind::Drawer,
            "t",
            "/topics/orders",
            html! { h2 id="t" { "x" } },
        )
        .into_string();
        assert_eq!(
            m,
            r#"<div class="ovl ovl-drawer root" data-overlay data-close="/topics/orders"><a class="scrim" href="/topics/orders" tabindex="-1" aria-hidden="true" data-overlay-close></a><div class="drawer-panel" role="dialog" aria-modal="true" aria-labelledby="t" tabindex="-1"><h2 id="t">x</h2></div></div>"#
        );
        let d = frame(Kind::Dialog, "t", "/p", html! {}).into_string();
        assert!(
            d.contains(r#"class="ovl ovl-dialog root""#)
                && d.contains(r#"<div class="modal" role="dialog""#)
        );
    }

    #[test]
    fn a_hostile_close_address_is_an_attribute_value_and_nothing_more() {
        let m = frame(Kind::Dialog, "t", r#""><script>x</script>"#, html! {}).into_string();
        assert!(!m.contains("<script>"), "{m}");
    }

    #[test]
    fn over_keeps_the_parent_and_the_overlay_apart() {
        let o = over(
            html! { p { "parent" } },
            frame(Kind::Dialog, "t", "/p", html! { h2 id="t" { "d" } }),
        );
        assert_eq!(o.main.into_string(), "<p>parent</p>");
        assert!(o.overlay.into_string().contains(r#"<h2 id="t">d</h2>"#));
    }

    #[test]
    fn close_is_a_link_a_script_can_take() {
        assert_eq!(
            close("btn", "/p", html! { "Close" }).into_string(),
            r#"<a class="btn" href="/p" data-dialog-close>Close</a>"#
        );
    }
}
