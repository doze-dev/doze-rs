//! The pages that say a page is not there: one that this edition does not have, and one that does not exist.

use maud::{Markup, html};

use crate::base::Base;
use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::Glyph;
use crate::ui::kafka::empty;
use crate::ui::layout::page_header;

/// What a screen the design draws and this edition does not build is, in a sentence that fits "{what} is not part of this
/// edition yet" (`Lab, fault injection for requests`).
#[must_use]
pub fn unavailable_what(name: &str) -> &'static str {
    match name {
        "Lab" => "Lab, fault injection for requests,",
        "Access" => "Access, users, credentials and ACLs,",
        "Settings" => "Settings, the console's own preferences,",
        "Producers" => "Producers, who writes to a topic and what its transactions hold,",
        "Log" => "Log, the segments and retention of a topic,",
        "Story" => "Story, what a group went through,",
        _ => "This page",
    }
}

/// A screen of the design that this edition does not have: one calm page that says so, with the way back. HTTP 200, as the
/// address is a good one; the shell marks every page `noindex`.
#[must_use]
pub fn unavailable(name: &str, back: &str, back_label: &str) -> Markup {
    html! {
        (page_header(name, html! {}, html! {}))
        (empty(
            Glyph::Info,
            "",
            None,
            html! { "Not available in this edition" },
            html! { (unavailable_what(name)) " is not part of this edition yet. Nothing is broken, and nothing you have is lost: this address will be its page." },
            html! { (link(back, Variant::Primary, Size::Normal, Some(Glyph::ArrowR), back_label)) },
        ))
    }
}

/// An address that is no page of the console. The path is shown as text, escaped like everything else.
#[must_use]
pub fn not_found(path: &str, base: &Base) -> Markup {
    html! {
        (page_header("Not found", html! { p class="sub" { "There is no page at " span class="mono" { (path) } "." } }, html! {}))
        div class="row" { (link(&base.url("/"), Variant::Primary, Size::Normal, Some(Glyph::ArrowR), "Back to Home")) }
    }
}
