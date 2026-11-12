//! The Live page: a snapshot of the newest requests, with a Refresh and, when a person asks, a tail (CON-800 to CON-812).
//!
//! The page is made of fixed parts. Under the title: a strip of metrics (region `live-stats`: the last minute, the rows kept,
//! how far back they reach, the clients, the refusals), the filters, the `Snapshot | Live tail` control, and the tape (region
//! `live-wire`: the table and its pager line). Nothing in it is a sentence that holds a number, no block comes or goes with the
//! data, and the table's columns are declared by the page. Every filter is a plain GET; a row's time is a link to its drawer.

use maud::{Markup, html};

use super::parts::{LIVE_FRAGMENT, LIVE_VIEW, request_url_in, wire_url};
use super::tape::Layout;
use crate::base::Base;
use crate::format::number;
use crate::model::{LiveState, LiveView};
use crate::ui::client;
use crate::ui::components::{Choice, Combobox, Select, combobox, select};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_live::{LiveGlyph, live_glyph};
use crate::ui::layout::page_header;
use crate::ui::metrics::{Stat, slot, stat_strip};
use crate::words::live::hhmm;

/// The region of the metrics.
pub const LIVE_STATS: &str = "live-stats";

/// The longest list a filter shows as a plain Select; a longer one is a Combobox.
const MOST_PLAIN: usize = 8;

/// A filter that is a Select: the first choice is "all" (an empty value), the rest are what the wire has seen. A value the filter
/// holds that the wire no longer shows stays a choice, so the page never lies about what it is filtering by.
fn select_chip(name: &str, all: &str, current: &str, options: &[String]) -> Markup {
    let mut choices = vec![Choice::new("", all)];
    choices.extend(options.iter().map(|o| Choice::new(o, o)));
    if !current.is_empty() && !options.iter().any(|o| o == current) {
        choices.push(Choice::new(current, current));
    }
    // A list that can grow (every app, every topic the wire has seen) is searchable.
    if choices.len() > MOST_PLAIN {
        return combobox(&Combobox {
            auto_submit: true,
            placeholder: all,
            class: if current.is_empty() {
                "dz-chip"
            } else {
                "dz-chip on"
            },
            ..Combobox::new(&format!("live-{name}"), name, all, &choices, current)
        });
    }
    select(&Select {
        auto_submit: true,
        ui: Some("filter"),
        placeholder: all,
        class: if current.is_empty() {
            "dz-chip"
        } else {
            "dz-chip on"
        },
        ..Select::new(&format!("live-{name}"), name, all, &choices, current)
    })
}

fn toggle(name: &str, on: bool, label: Markup) -> Markup {
    html! {
        button class={"fchip" @if on { " on" }} type="submit" name=(name) value=(if on { "0" } else { "1" }) data-ui="filter" aria-pressed=(on) { (label) }
    }
}

fn toolbar(v: &LiveView, base: &Base) -> Markup {
    let q = &v.query;
    html! {
        form class="toolbar" method="get" action=(base.url("/live")) data-live-filters
            hx-get=(base.url("/live")) hx-target={"#" (LIVE_VIEW)} hx-select={"#" (LIVE_VIEW)} hx-swap=(client::IN_PLACE_SWAP) hx-push-url="true" {
            input type="submit" value="Filter" aria-label="Filter the wire" hidden data-scaffold;
            (crate::ui::inplace::search_box(LIVE_VIEW, &base.url("/live"), "q", "Filter by client, API or topic", "Filter the wire", &q.text))
            (select_chip("client", "All apps", &q.client, &v.clients_ids))
            (select_chip("topic", "All topics", &q.topic, &v.topics_seen))
            // A toggle's own button carries its new value; the hidden fields keep the other's state.
            (toggle("refused", q.refused_only, html! { (glyph(Glyph::Filter, 14, "")) "Refused only" }))
            // "Idle polls hidden" is on by default: its button says what a click does by what it is now.
            button class={"fchip" @if q.hide_idle { " on" }} type="submit" name="idle" value=(if q.hide_idle { "0" } else { "1" }) data-ui="toggle" aria-pressed=(q.hide_idle) { "Idle polls hidden" }
            @if !q.api.is_empty() { input type="hidden" name="api" value=(q.api); }
            @if !q.group.is_empty() { input type="hidden" name="group" value=(q.group); }
            span class="sp" {}
            (client::dialog_link("btn sm", &format!("{}{}", base.url("/live/clients"), q.to_query()), html! { (live_glyph(LiveGlyph::Network, 16)) "Clients" }))
        }
    }
}

/// The metrics and the refusals line: one region, one fixed height.
fn stats_region(v: &LiveView, base: &Base) -> Markup {
    let reach = v.window.oldest_ms.map_or_else(|| "none".to_owned(), hhmm);
    let strip = stat_strip(&[
        Stat::new("Requests, last minute", number(i64::from(v.last_minute))).ch(7),
        Stat::new(
            "Rows kept",
            number(i64::try_from(v.window.rows).unwrap_or(0)),
        )
        .unit(format!(
            "of {}",
            number(i64::try_from(v.window.capacity).unwrap_or(0))
        ))
        .ch(6),
        Stat::new("Reaches back to", reach)
            .unit("UTC")
            .tip("The oldest row kept; the wire starts empty at every start")
            .mono()
            .ch(5),
        Stat::new("Clients", number(i64::try_from(v.clients).unwrap_or(0))).ch(5),
    ]);
    html! {
        (strip)
        (refusals(v, base))
    }
}

/// The refusals, folded: one line for each (code, client) with how many and when the last was, at most three and "and N more".
/// A strip of one line that opens in place (the Live toolbar and the tape stay where they are while it is shut), and says so when
/// there is nothing to say: it is there either way (CON-807).
fn refusals(v: &LiveView, base: &Base) -> Markup {
    let total = v.refused_total + v.denied_total;
    html! {
        details class="refusals" {
            summary {
                @if total == 0 { span class="dim" { "No refusals among the rows kept." } }
                @else { b { "Refusals" } span class="dim" { " among the rows kept: " } span class="mono" { (number(i64::try_from(total).unwrap_or(0))) } }
            }
            @if total > 0 {
                ul class="refusal-lines" {
                    @for r in &v.refused {
                        li {
                            span class="mono" { (r.code_name) " (" (r.code) ")" } " · " (r.client) " · "
                            span class="mono" { "×" (number(i64::from(r.count))) } " · last " time class="mono" datetime=(crate::format::iso(r.at_ms)) title=(crate::format::exact(r.at_ms)) data-rel { (crate::format::clock(r.at_ms)) }
                            " · " a class="lnk" href=(request_url_in(base, r.id, &v.query)) data-dialog { "View" }
                        }
                    }
                    @if v.refused_more > 0 { li class="dim" { "and " (v.refused_more) " more kinds" } }
                }
            }
        }
    }
}

/// The stats region as a fragment.
#[must_use]
pub fn stats_fragment(v: &LiveView, base: &Base) -> Markup {
    client::fragment(
        LIVE_STATS,
        &wire_url(base, LIVE_STATS, &v.query.to_query()),
        stats_region(v, base),
    )
}

fn pager_row(v: &LiveView, base: &Base) -> Markup {
    let mut newer = v.query.clone();
    newer.before = None;
    let older = v.older.map(|id| {
        let mut q = v.query.clone();
        q.before = Some(id);
        format!("{}{}", base.url("/live"), q.to_query())
    });
    let shown = number(i64::try_from(v.rows.len()).unwrap_or(0));
    let matched = number(i64::try_from(v.matched).unwrap_or(0));
    let line = html! {
        "Showing " (slot(&shown, 4)) " of " (slot(&matched, 6)) " matching rows" @if v.query.hide_idle { " · idle polls hidden" }
    };
    html! {
        div class="pager" {
            span class="nowrap" { (line) }
            span {
                @if v.query.before.is_some() {
                    a class="btn sm" href={(base.url("/live")) (newer.to_query())} data-ui="page" { (live_glyph(LiveGlyph::ChevL, 14)) "Newer" }
                } @else {
                    button class="btn sm" type="button" data-ui="page" disabled { (live_glyph(LiveGlyph::ChevL, 14)) "Newer" }
                }
                @if let Some(href) = &older {
                    a class="btn sm" href=(href) data-ui="page" { "Older" (live_glyph(LiveGlyph::ChevR, 14)) }
                } @else {
                    button class="btn sm" type="button" data-ui="page" disabled { "Older" (live_glyph(LiveGlyph::ChevR, 14)) }
                }
            }
        }
    }
}

/// The tape as a fragment: the table and its pager. It is the tail's region: `data-tail-url` is where a person's Live tail
/// listens, from `data-cursor` on.
#[must_use]
pub fn live_fragment(v: &LiveView, base: &Base) -> Markup {
    let url = wire_url(base, LIVE_FRAGMENT, &v.query.to_query());
    let stream = crate::form::encode_query(&[
        ("tail", "live"),
        ("q", &v.query.text),
        ("client", &v.query.client),
        ("api", &v.query.api),
        ("topic", &v.query.topic),
        ("group", &v.query.group),
        ("refused", if v.query.refused_only { "1" } else { "" }),
        ("idle", if v.query.hide_idle { "" } else { "0" }),
        ("since", &v.cursor.to_string()),
    ]);
    let events = base.url(&format!("/events{stream}"));
    let empty = format!(
        "No requests yet. Send one with any Kafka client, for example: echo hi | kcat -P -b {} -t orders -X client.id=admin-cli",
        v.listen
    );
    let t = super::tape::table_with_empty(&v.rows, base, &v.query, Layout::Live, &empty);
    client::fragment(
        LIVE_FRAGMENT,
        &url,
        html! {
            div class="tail-root" data-tail-url=(events) data-cursor=(v.cursor) data-tail-rows="tape" {
                (t)
                (pager_row(v, base))
            }
        },
    )
}

/// The Live page's main content.
#[must_use]
pub fn live_page(v: &LiveView, base: &Base) -> Markup {
    html! {
        (page_header(
            "Live",
            html! { p class="sub" { "Everything your apps send, newest first. Repeats fold into one row with a count; the oldest rows fall off." } },
            html! {},
        ))
        (stats_fragment(v, base))
        (crate::ui::inplace::region(LIVE_VIEW, html! {
            (toolbar(v, base))
            (crate::ui::tail::tail_bar(LIVE_FRAGMENT, true))
            (live_fragment(v, base))
        }))
    }
}

/// The last step of the breadcrumb when the page is in a state the boards name; none: the page is the same in every state.
#[must_use]
pub fn state_crumb(v: &LiveView) -> Option<&'static str> {
    match v.state {
        LiveState::Quiet => Some("Nothing yet"),
        LiveState::Busy => Some("A busy minute"),
        LiveState::Steady => None,
    }
}
