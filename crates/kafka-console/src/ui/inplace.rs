//! In-place controls: a chip, a pager, a sort header, a toolbar and a search box that change ONE region of their own page.
//!
//! The foundation (`ui::client`) gives a plain link or GET form the attributes of an in-place swap: the answer is the page's
//! region, the element with the region's id is picked out of it and morphed into place (scroll, focus and typed text stay,
//! only what changed fades), the address is pushed. These helpers add what the pages need on top: the label of a chip, the id
//! of a toolbar, and a filter box that asks again as you type (debounced) and replaces the history entry instead of adding one.
//!
//! The rule for a page: a region is a stable id on a wrapper (`div.frag#topics-view`) that holds the toolbar AND the data it
//! drives, so that a chip that changes the toolbar's own state (which chip is on, a count, a hidden field) changes it with the
//! data. A control inside the region keeps its node (the morph matches it), so the focus stays on it. Everything here is also
//! a plain link or a plain GET form: with no script the browser follows it.

use maud::{Markup, html};

use super::client::IN_PLACE_SWAP;
use super::glyph::{Glyph, glyph};

/// The request header a control sends to say "replace the history entry, do not add one" (typing in a search box).
pub const HEADER_HISTORY: &str = "x-history";
/// The value of [`HEADER_HISTORY`] that asks for a replacement.
pub const HISTORY_REPLACE: &str = "replace";

/// The header of a typing request, as the attribute says it.
const REPLACING: &str = r#"{"x-history":"replace"}"#;

/// A wrapper that is the region `id` of a page: the part an in-place control of that page asks for. It lays out as its children
/// do (`display: contents`).
#[must_use]
pub fn region(id: &str, content: Markup) -> Markup {
    html! { div class="frag" id=(id) { (content) } }
}

/// The attributes a link may carry besides what makes it in place: the kit's control kind (`data-ui`), the API it calls
/// (`data-op`) and the name of a chip whose text is a state (`aria-label`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Opts<'a> {
    pub label: Option<&'a str>,
    pub ui: Option<&'a str>,
    pub op: Option<&'a str>,
}

/// A link (a chip, a pager, a header) that changes only the region `target`. A `#anchor` stays in the link a browser follows with
/// no script and is left out of the request (it would scroll the page to it, which is what an in-place control must not do).
#[must_use]
pub fn link_with(target: &str, class: &str, href: &str, o: &Opts, content: Markup) -> Markup {
    let get = href.split('#').next().unwrap_or(href);
    html! {
        a class=(class) href=(href) aria-label=[o.label] data-ui=[o.ui] data-op=[o.op] hx-get=(get) hx-target={"#" (target)} hx-select={"#" (target)}
            hx-swap=(IN_PLACE_SWAP) hx-push-url="true" { (content) }
    }
}

/// [`link_with`] with nothing besides (`label` names a chip whose text is a state).
#[must_use]
pub fn link(target: &str, class: &str, href: &str, label: Option<&str>, content: Markup) -> Markup {
    link_with(
        target,
        class,
        href,
        &Opts {
            label,
            ..Opts::default()
        },
        content,
    )
}

/// A chip of a group of chips (a filter with one choice on): the one that is on is the current one (`aria-current`).
#[must_use]
pub fn chip_link(target: &str, class: &str, href: &str, on: bool, content: Markup) -> Markup {
    html! {
        a class=(class) href=(href) aria-current=[on.then_some("true")] hx-get=(href) hx-target={"#" (target)} hx-select={"#" (target)}
            hx-swap=(IN_PLACE_SWAP) hx-push-url="true" { (content) }
    }
}

/// A GET form (a toolbar) that changes only the region `target`; the submit buttons inside it are the controls. `id` names the
/// form for the scripts and the tests that read it.
#[must_use]
pub fn form(
    target: &str,
    id: Option<&str>,
    class: &str,
    style: Option<&str>,
    action: &str,
    content: Markup,
) -> Markup {
    html! {
        form class=(class) id=[id] style=[style] method="get" action=(action) hx-get=(action) hx-target={"#" (target)} hx-select={"#" (target)}
            hx-swap=(IN_PLACE_SWAP) hx-push-url="true" hx-trigger="submit" { (content) }
    }
}

/// The filter box of a toolbar built with [`form`] that asks again as you type: 250 ms after the last key, or at once on
/// Enter. Typing replaces the history entry (a filter you are still typing is not a place to go back to), and a newer request
/// replaces an older one in flight. The rest of the form (the chips that are on, hidden fields) travels with it.
#[must_use]
pub fn search_box(
    target: &str,
    action: &str,
    name: &str,
    placeholder: &str,
    label: &str,
    value: &str,
) -> Markup {
    html! {
        div class="finput" {
            (glyph(Glyph::Search, 16, ""))
            (search_input(&Search { target, action, name, id: None, placeholder, label, value }))
        }
    }
}

/// The input of [`search_box`] alone, for a toolbar that draws its own frame around it.
#[derive(Debug, Clone, Copy)]
pub struct Search<'a> {
    pub target: &'a str,
    pub action: &'a str,
    pub name: &'a str,
    pub id: Option<&'a str>,
    pub placeholder: &'a str,
    pub label: &'a str,
    pub value: &'a str,
}

/// The input of a [`Search`].
#[must_use]
pub fn search_input(s: &Search<'_>) -> Markup {
    html! {
        input type="text" id=[s.id] name=(s.name) placeholder=(s.placeholder) value=(s.value) aria-label=(s.label) autocomplete="off" spellcheck="false"
            hx-get=(s.action) hx-include="closest form" hx-trigger="input changed delay:250ms, search" hx-sync="this:replace"
            hx-target={"#" (s.target)} hx-select={"#" (s.target)} hx-swap=(IN_PLACE_SWAP) hx-push-url="false" hx-replace-url="true"
            hx-headers=(REPLACING);
    }
}

#[cfg(test)]
mod tests;
