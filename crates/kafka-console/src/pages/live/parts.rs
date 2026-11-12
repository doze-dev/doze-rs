//! What the Live pages share: the fragment's address, the result cell, the words of a class, a link that filters.

use maud::{Markup, html};

use crate::base::Base;
use crate::model::{WireClass, WireQuery, WireRow};

/// The id of the Live fragment.
pub const LIVE_FRAGMENT: &str = crate::live_ids::LIVE_WIRE;

/// Where a wire fragment is fetched: the fragment's address with the filters.
#[must_use]
pub fn wire_url(base: &Base, id: &str, query: &str) -> String {
    base.url(&format!("/fragments/{id}{query}"))
}

/// The address of one request's drawer.
#[must_use]
pub(super) fn request_url(base: &Base, id: u64) -> String {
    base.url(&format!("/live/{id}"))
}

/// The address of one request's drawer over the Live page as the filters `q` have it: closing the drawer returns to that page.
#[must_use]
pub(super) fn request_url_in(base: &Base, id: u64, q: &crate::model::WireQuery) -> String {
    format!("{}{}", request_url(base, id), q.to_query())
}

/// The word of a result, as the boards say it.
#[must_use]
pub(super) fn class_word(class: WireClass, code: i16) -> &'static str {
    match class {
        WireClass::Done => "Done",
        WireClass::Told if matches!(code, 27 | 79 | 110 | 113) => "Told to rejoin",
        WireClass::Told => "Told to retry",
        WireClass::Failed => "Failed",
        WireClass::Refused => "Refused",
        WireClass::Denied => "Denied",
        WireClass::Closed => "Closed",
    }
}

/// The colour class of a state word: Denied is the amber of `warn`, Kafka's refusals the violet of `refused`.
#[must_use]
pub(super) fn class_css(class: WireClass) -> &'static str {
    match class {
        WireClass::Done => "ok",
        WireClass::Told | WireClass::Denied => "warn",
        WireClass::Failed | WireClass::Closed => "bad",
        WireClass::Refused => "refused",
    }
}

/// The result cell: the word (`st`, or `rw` in the docks), what it adds, and for an error the code and the broker's words.
#[must_use]
pub(super) fn result_cell(r: &WireRow, word_class: &str) -> Markup {
    let word = class_word(r.class, r.code);
    let css = class_css(r.class);
    html! {
        span class={(word_class) " " (css)} { (word) }
        @if r.class == WireClass::Done || r.class == WireClass::Closed && r.code == 0 {
            @if !r.detail.is_empty() { " " span class="dim" { (r.detail) } }
        } @else {
            " " span class="mono" { (r.code_name) " (" (r.code) ")" }
            @if !r.detail.is_empty() { span class="sub" { (r.detail) } }
        }
    }
}

/// A cell's text that is also a link to the same page filtered by it (a click on a client, an API or a topic).
#[must_use]
pub(super) fn filter_link(
    base: &Base,
    q: &WireQuery,
    field: &str,
    value: &str,
    label: Markup,
) -> Markup {
    let href = format!("{}{}", base.url("/live"), q.with(field, value).to_query());
    html! {
        a class="cell-filter" href=(href) title={"Show only " (value)} hx-get=(href) hx-target={"#" (LIVE_FRAGMENT)} hx-select={"#" (LIVE_FRAGMENT)}
            hx-swap=(crate::ui::client::IN_PLACE_SWAP) hx-push-url="true" preload="false" { (label) }
    }
}
