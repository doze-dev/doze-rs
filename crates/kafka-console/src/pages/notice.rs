//! The two pages that say a page is not there: one that is not built yet, and one that does not exist.

use maud::{Markup, html};

use crate::base::Base;
use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::Glyph;
use crate::ui::layout::page_header;
use crate::ui::notice::{Tone, callout};

/// A screen of the design that this build does not have yet: it says so and offers the way back.
#[must_use]
pub fn planned(title: &str, base: &Base) -> Markup {
    html! {
        (page_header(title, html! { p class="sub" { "This part of the console is not built yet." } }, html! {}))
        (callout(Tone::Plain, Glyph::Info, html! {
            b { "Not built yet." } " Home and Connect are here; the rest of the console arrives in later releases, and this address will be its page."
        }))
        div class="row" style="margin-top:24px" { (link(&base.url("/"), Variant::Primary, Size::Normal, Some(Glyph::ArrowR), "Back to Home")) }
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
