//! The docks: the same tape filtered to one topic or one group. A dock is a fragment (so it follows the recorder) and a page
//! that opens it up (boards `topic-wire`, `group-wire`). The console's own requests never reach the recorder, so a dock can be
//! quieter than the topic's real traffic: it says so.

use maud::{Markup, html};

use super::parts::{request_url, wire_url};
use super::tape::{Layout, table};
use crate::base::{Base, segment};
use crate::format::clock;
use crate::model::{GroupWireView, WireClass, WireQuery, WireRow, WireWindow};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::glyph_live::{LiveGlyph, live_glyph};
use crate::ui::metrics::slot;
use crate::ui::notice::{Tone, callout};
use crate::words::live::hhmm;

/// The region of a topic's dock.
pub const TOPIC_WIRE: &str = "topic-wire";
/// The region of a group's wire.
pub const GROUP_WIRE: &str = "group-wire";

/// What a dock says of how far back it reaches: slots, so that the line never changes length.
fn reach_line(w: &WireWindow) -> Markup {
    let rows = crate::format::number(i64::try_from(w.rows).unwrap_or(0));
    let back = w.oldest_ms.map_or_else(|| "none".to_owned(), hhmm);
    html! { "Among the last " (slot(&rows, 5)) " rows the broker kept, back to " (slot(&back, 5)) " UTC." }
}

fn expanded_toolbar(topic: &str) -> Markup {
    html! {
        div class="toolbar" {
            span class="fchip on" { "Topic " (topic) }
            span class="fchip" { "Client" }
            span class="fchip" { "API" }
            span class="fchip" { "Refused only" }
        }
    }
}

/// A topic's "On the wire" dock: the last requests that touched it. `expanded` is the page of its own (`topic-wire`).
#[must_use]
pub fn topic_dock(
    topic: &str,
    rows: &[WireRow],
    w: &WireWindow,
    base: &Base,
    expanded: bool,
) -> Markup {
    let q = WireQuery {
        topic: vec![topic.to_owned()],
        hide_idle: false,
        ..WireQuery::default()
    };
    let url = wire_url(base, TOPIC_WIRE, &format!("?topic={}", segment(topic)));
    let t = base.url(&format!("/topics/{}", segment(topic)));
    client::fragment(
        TOPIC_WIRE,
        &url,
        html! {
            div class="wire-dock" data-extra="wire-dock" {
                div class="row sb" {
                    div {
                        h2 { "On the wire" @if expanded { ": " (topic) } }
                        p class="lede" { "Client requests that touched this topic. " (reach_line(w)) " A Fetch that returns nothing is folded." }
                    }
                    @if expanded { a class="btn sm" href=(t) { "Collapse to the dock" } }
                    @else { a class="btn sm" href={(t) "/wire"} { "Expand the wire" } }
                }
                @if expanded { (expanded_toolbar(topic)) }
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

fn chip(on: bool, name: &str, value: &str, label: &str, count: Option<usize>) -> Markup {
    html! {
        button class={"fchip" @if on { " on" }} type="submit" name=(name) value=(value) data-ui="filter" aria-pressed=(on) {
            (label) @if let Some(n) = count { span class="dim" { (n) } }
        }
    }
}

fn group_toolbar(v: &GroupWireView, base: &Base) -> Markup {
    let q = &v.query;
    html! {
        form class="toolbar" method="get" action=(base.url(&format!("/groups/{}/wire", segment(&v.group)))) {
            input type="submit" value="Filter" aria-label="Filter the wire" hidden data-scaffold;
            (chip(q.kind.is_empty() && !q.refused_only, "kind", "", "All", Some(v.all)))
            (chip(q.kind == "join", "kind", "join", "Join and sync", None))
            (chip(q.kind == "commits", "kind", "commits", "Commits", None))
            (chip(q.kind == "heartbeats", "kind", "heartbeats", "Heartbeats", None))
            (chip(q.refused_only, "refused", "1", "Refused", Some(v.refused)))
            span class="sp" {}
            (crate::ui::kafka::filter_box("q", "Filter by member or client", "Filter by member or client", &q.text))
        }
    }
}

/// A group's wire page: every client request that touched the group, newest first. The group pages come from slice 9c: this is
/// its body only, routed at `/groups/{group}/wire`; the page header of the group (its state and pills) is theirs to put above.
#[must_use]
pub fn group_wire_fragment(v: &GroupWireView, base: &Base) -> Markup {
    let group = v.group.as_str();
    let q = WireQuery {
        group: group.to_owned(),
        hide_idle: false,
        ..v.query.clone()
    };
    let query = crate::form::encode_query(&[
        ("kind", &v.query.kind),
        ("refused", if v.query.refused_only { "1" } else { "" }),
        ("q", &v.query.text),
    ]);
    let url = wire_url(
        base,
        GROUP_WIRE,
        &format!(
            "{}{}group={}",
            query,
            if query.is_empty() { "?" } else { "&" },
            segment(group)
        ),
    );
    client::fragment(
        GROUP_WIRE,
        &url,
        html! {
            (table(&v.rows, base, &q, Layout::Group))
            p class="help" style="margin-top:12px" { (reach_line(&v.window)) }
            (refusal_note(&v.rows, base))
        },
    )
}

/// The page: the title, the filters and the region.
#[must_use]
pub fn group_wire_page(v: &GroupWireView, base: &Base) -> Markup {
    let group = v.group.as_str();
    html! {
        div class="page-h" {
            div {
                div class="title-row" { h1 class="mono" { (group) } }
                p class="sub" {
                    "Every client request that touched " (group) ", newest first. Heartbeats fold into one row; a refusal stays on its own row. The console's own requests are not listed."
                }
            }
            div class="acts" {
                a class="btn" href=(base.url(&format!("/groups/{}", segment(group)))) { (live_glyph(LiveGlyph::Back, 16)) "Back to the group" }
            }
        }
        (group_toolbar(v, base))
        (group_wire_fragment(v, base))
    }
}
