//! The docks: the same tape filtered to one topic or one group. A dock is a fragment (so it follows the recorder) and a page
//! that opens it up (boards `topic-wire`, `group-wire`). The console's own requests never reach the recorder, so a dock can be
//! quieter than the topic's real traffic: it says so.

use maud::{Markup, html};

use super::parts::{request_url, wire_url};
use super::tape::{Layout, table};
use crate::base::{Base, segment};
use crate::format::clock;
use crate::live_ids::{GROUP_WIRE, TOPIC_WIRE};
use crate::model::{WireClass, WireQuery, WireRow, WireWindow};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::glyph_live::{LiveGlyph, live_glyph};
use crate::ui::notice::{Tone, callout};
use crate::words::live::{hhmm, reach};

/// What a dock says of how far back it reaches.
fn reach_line(w: &WireWindow, now_ms: i64) -> String {
    match (w.oldest_ms, reach(w, now_ms)) {
        (Some(o), Some(_)) => format!(
            "among the last {} rows the broker kept (back to {})",
            w.rows,
            hhmm(o)
        ),
        _ => "none yet: the wire starts empty at every start".to_owned(),
    }
}

/// A topic's "On the wire" dock: the last requests that touched it.
#[must_use]
pub fn topic_dock(
    topic: &str,
    rows: &[WireRow],
    w: &WireWindow,
    now_ms: i64,
    base: &Base,
    expanded: bool,
) -> Markup {
    let q = WireQuery {
        topic: topic.to_owned(),
        hide_idle: false,
        ..WireQuery::default()
    };
    let url = wire_url(base, TOPIC_WIRE, &format!("?topic={}", segment(topic)));
    client::fragment(
        TOPIC_WIRE,
        &url,
        html! {
            div class="wire-dock" data-extra="wire-dock" {
                div class="row sb" {
                    div {
                        h2 { "On the wire: " (topic) }
                        p class="lede" { "Client requests that touched this topic, " (reach_line(w, now_ms)) ". A Fetch that returns nothing is folded." }
                    }
                    @if expanded {
                        a class="btn sm" href=(base.url(&format!("/topics/{}", segment(topic)))) { "Collapse to the dock" }
                    } @else {
                        a class="btn sm" href=(base.url(&format!("/topics/{}/wire", segment(topic)))) { "Expand the wire" }
                    }
                }
                @if expanded {
                    div class="toolbar" {
                        span class="fchip on" { "Topic " (topic) }
                        a class="fchip" href=(format!("{}?topic={}", base.url("/live"), segment(topic))) { "Client" }
                        a class="fchip" href=(format!("{}?topic={}", base.url("/live"), segment(topic))) { "API" }
                        a class="fchip" href=(format!("{}?topic={}&refused=1", base.url("/live"), segment(topic))) { "Refused only" }
                        span class="sp" {}
                        a class="btn sm" href=(format!("{}?topic={}", base.url("/live"), segment(topic))) { "Open in Live" }
                    }
                }
                @if rows.is_empty() {
                    p class="help" { "No client has touched " span class="mono" { (topic) } " since the broker started." }
                } @else { (table(rows, base, &q, Layout::Topic)) }
                p class="help" style="margin-top:16px" {
                    "Kept in memory and emptied at every restart. The console's own requests are not listed, so this can be quieter than the topic's real traffic. A row opens to its detail in "
                    a class="lnk" href=(base.url("/live")) { "Live" } "."
                }
            }
        },
    )
}

/// The refusal under a group's wire: the newest refused row, in words.
fn refusal_note(rows: &[WireRow], base: &Base) -> Markup {
    let Some(r) = rows
        .iter()
        .find(|r| matches!(r.class, WireClass::Refused | WireClass::Denied))
    else {
        return html! {};
    };
    html! {
        div style="margin-top:24px" { (callout(Tone::Refused, Glyph::X, html! {
            b { (r.code_name) " (" (r.code) ")" } " at " span data-vary { (clock(r.at_ms)) } ": "
            @if r.detail.is_empty() { "refused." } @else { (r.detail) }
            " " a class="lnk" href=(request_url(base, r.id)) { "Open the request" }
        })) }
    }
}

/// A group's wire page: every client request that touched the group, newest first. The group pages come from slice 9c: this is
/// its body only, routed at `/groups/{group}/wire`; the page header of the group (its state and pills) is theirs to put above.
#[must_use]
pub fn group_wire_page(
    group: &str,
    rows: &[WireRow],
    w: &WireWindow,
    now_ms: i64,
    base: &Base,
) -> Markup {
    let q = WireQuery {
        group: group.to_owned(),
        hide_idle: false,
        ..WireQuery::default()
    };
    let url = wire_url(base, GROUP_WIRE, &format!("?group={}", segment(group)));
    client::fragment(
        GROUP_WIRE,
        &url,
        html! {
            div class="page-h" {
                div {
                    div class="title-row" { h1 class="mono" { (group) } }
                    p class="sub" data-extra="live-sentence" aria-live="polite" {
                        "Every client request that touched " (group) " " (reach_line(w, now_ms)) ", newest first. Heartbeats fold into one row; a refusal stays on its own row. The console's own requests are not listed."
                    }
                }
                div class="acts" {
                    a class="btn" href=(base.url(&format!("/groups/{}", segment(group)))) { (live_glyph(LiveGlyph::Back, 16)) "Back to the group" }
                }
            }
            div data-extra="wire-dock" {
                @if rows.is_empty() {
                    p class="help" { "No client has touched this group since the broker started." }
                } @else { (table(rows, base, &q, Layout::Group)) }
            }
            (refusal_note(rows, base))
        },
    )
}
