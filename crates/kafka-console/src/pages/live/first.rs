//! The page a new client lands on (board `connect-first-request`): who it is, what it asked in order, and what to do next.

use maud::{Markup, html};

use super::tape::{Layout, table};
use crate::base::Base;
use crate::format::clock_ms;
use crate::model::{FirstView, WireQuery};
use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::{card, kv, page_header};

fn banner(v: &FirstView) -> Markup {
    html! {
        div class="banner" { (glyph(Glyph::Check, 18, ""))
            span {
                @if v.all_answered { b { "It works." } " The client reached the listener, negotiated versions and was answered. Nothing it asked for was refused." }
                @else { b { "It reached the broker." } " " (v.refused) " of its requests " @if v.refused == 1 { "was" } @else { "were" } " refused or not answered: open the row for Kafka's words." }
            }
        }
    }
}

fn story(v: &FirstView) -> Markup {
    html! {
        div class="story" style="margin-top:16px" {
            @for r in v.rows.iter().rev() {
                div class="ev" { div class="tm" data-vary { (clock_ms(r.at_ms)) } div class="dot" { i {} }
                    div class="tx" { b { (r.api) " v" (r.version) } br; (r.target) @if !r.detail.is_empty() { " · " (r.detail) } } }
            }
            div class="ev idle" { div class="tm" { "next" } div class="dot" { i {} } div class="tx dim" { "Waiting for its next request: a produce, a fetch or a group call." } }
        }
    }
}

/// The page. `v.listen` and `v.profile` are filled by the caller (the recorder does not know them).
#[must_use]
pub fn first_page(v: &FirstView, base: &Base) -> Markup {
    let versions = v
        .versions
        .iter()
        .map(|(a, n)| format!("{a} v{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    html! {
        (page_header(
            "A new client just connected",
            html! { p class="sub" data-extra="live-sentence" aria-live="polite" {
                b class="mono" { (v.client) } " sent its first request at " span data-vary { (clock_ms(v.first_ms)) }
                @if let Some(p) = &v.peer { ", from " (p) } "."
                @if let Some(s) = &v.software { " It speaks Kafka as " span class="mono" { (s) } "." }
            } },
            link(&base.url("/live"), Variant::Primary, Size::Normal, Some(Glyph::Live), "Open Live"),
        ))
        (banner(v))
        div class="g2" style="align-items:start" {
            (card(html! { h2 { "Who it is" } (kv(vec![
                ("client.id", html! { span class="mono" { (v.client) } }),
                ("Software", html! { (v.software.as_deref().unwrap_or("not announced")) " " span class="dim" { "(from ApiVersions)" } }),
                ("From", html! { span class="mono" { (v.peer.as_deref().unwrap_or("-")) } }),
                ("First request", html! { span class="mono" data-vary { (clock_ms(v.first_ms)) } }),
                ("Version range", html! { (versions) span class="sub" { "Each API is spoken at the highest version both sides know." } }),
            ])) }))
            div class="card" data-extra="story" { h2 { "What it asked, in order" } (story(v)) }
        }
        div class="sec" style="margin-top:40px" {
            h2 { "The wire for this client" }
            p class="lede" { "The same rows Live shows, filtered to " span class="mono" { (v.client) } "." }
            (table(&v.rows, base, &WireQuery::default(), Layout::First))
            div class="callout" style="margin-top:24px" { (glyph(Glyph::Info, 18, ""))
                div { b { "Next:" } " send a record and watch it land. "
                    span class="mono" { "echo hi | kcat -P -b " (v.listen) " -t orders -X client.id=" (v.client) }
                    ", then open " a class="lnk" href=(base.url("/topics")) { "Topics" } "." } }
        }
    }
}
