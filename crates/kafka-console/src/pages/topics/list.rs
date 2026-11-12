//! The topics list in its three states: `topics-list`, `topics-none` and `topics-filtered`.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::model::{TopicRow, TopicsState, TopicsView};
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::inplace;
use crate::ui::kafka::{Tab, empty, pager_with, sec, tabs};
use crate::ui::layout::page_header;
use crate::ui::notice::{Tone, callout};
use crate::ui::metrics::{Stat, slot, stat_strip};

use super::list_table::{internal_table, table};

/// The regions of the list: the metrics and the table.
pub const LIST_FRAGMENT: &str = "topics-table";
pub const LIST_STATS: &str = "topics-stats";

/// The line under the table, with its numbers in slots.
fn shown(v: &TopicsView) -> Markup {
    let n = v.rows.iter().filter(|r| !r.internal).count();
    let of = if v.filter.is_empty() { v.total } else { n };
    let num = |n: usize| crate::format::number(i64::try_from(n).unwrap_or(0));
    html! {
        @if v.truncated { "First " (slot(&num(n), 5)) " of " (slot(&num(v.total), 5)) " topics, by name" } @else { (slot(&num(n), 5)) " of " (slot(&num(of), 5)) " topics" }
    }
}

/// The metrics of the list, as a fragment: no sentence holds a number.
#[must_use]
pub fn stats_fragment(v: &TopicsView, base: &Base, query: &str) -> Markup {
    let num = |n: usize| crate::format::number(i64::try_from(n).unwrap_or(0));
    let mut stats = vec![
        Stat::new("Topics", num(v.total)).ch(5),
        Stat::new("Partitions", num(v.partitions)).ch(5),
    ];
    match &v.newest {
        Some((topic, ms)) => {
            stats.push(Stat::at("Newest write", *ms, v.now_ms));
            stats.push(Stat::new("Written to", topic.clone()).mono().ch(20));
        }
        None => {
            stats.push(Stat::new("Newest write", "none yet").mono().ch(11));
            stats.push(Stat::new("Written to", "none").mono().ch(20));
        }
    }
    client::fragment(
        LIST_STATS,
        &base.url(&format!("/fragments/{LIST_STATS}{query}")),
        stat_strip(&stats),
    )
}

/// The table and the line under it, as a fragment.
#[must_use]
pub fn list_fragment(v: &TopicsView, base: &Base, query: &str) -> Markup {
    client::fragment(
        LIST_FRAGMENT,
        &base.url(&format!("/fragments/{LIST_FRAGMENT}{query}")),
        html! { (table(v, base)) (pager_with(shown(v), html! {})) },
    )
}

/// The id of the region the toolbar and the list share: a filter, a chip or Refresh changes it and nothing else of the page.
pub(super) const LIST_VIEW: &str = "topics-view";

fn toolbar(v: &TopicsView, base: &Base) -> Markup {
    let action = base.url("/topics");
    inplace::form(
        LIST_VIEW,
        None,
        "toolbar",
        None,
        &action,
        html! {
                // The button Enter presses in the filter box: not a control of the page, so it is out of the board's reckoning.
                input type="submit" value="Filter" aria-label="Filter topics" hidden data-scaffold;
                (inplace::search_box(LIST_VIEW, &action, "q", "Filter topics by name", "Filter topics by name", &v.filter))
                button class={"fchip" @if v.show_internal { " on" }} type="submit" name="internal" value=(if v.show_internal { "0" } else { "1" }) data-ui="toggle" aria-pressed=(v.show_internal) {
                    span class={"toggle" @if v.show_internal { " on" }} {} "Show internal"
                }
                @if v.show_internal { input type="hidden" name="internal" value="1"; }
        },
    )
}

fn internal_note(v: &TopicsView, base: &Base) -> Markup {
    let row = TopicRow::internal();
    html! {
        div style="margin-top:24px" {
            (callout(Tone::Plain, Glyph::Info, html! {
                div style="flex:1" {
                    b { @if v.show_internal { "The internal topic is shown." } @else { "One internal topic is hidden." } }
                    " Turn on " b { "Show internal" } " and this row joins the list:"
                    div style="margin-top:12px" { (internal_table(&row, base)) }
                    p style="margin:12px 0 0" {
                        b { "Why it is read only." } " " span class="mono" { "__consumer_offsets" }
                        " is Kafka's own name for the place the group coordinator keeps each group's committed offsets, spread over 50 partitions. This broker keeps commits in its own store and answers Kafka's offset calls from it, so there is no log here to read, produce to, delete from, or change. The name is shown because Kafka clients and tools know it; the 50 partitions decide which coordinator slot a group lands in. Look at a group's commits on its Lag tab instead: "
                        a class="lnk" href=(base.url("/groups")) { "open Groups" } "."
                    }
                }
            }))
        }
    }
}

fn head(v: &TopicsView, base: &Base, sub: &str) -> Markup {
    html! {
        (page_header(
            "Topics",
            html! { p class="sub" { (sub) } },
            html! { (link(&base.url("/topics/new"), Variant::Primary, Size::Normal, Some(Glyph::Plus), "Create topic")) },
        ))
        (tabs(&[
            Tab { label: "Topics", href: base.url("/topics"), on: true, count: Some(v.total) },
            Tab { label: "Groups", href: base.url("/groups"), on: false, count: Some(v.groups) },
        ]))
    }
}

fn none(v: &TopicsView, base: &Base, listen: &str) -> Markup {
    let create = || {
        link(
            &base.url("/topics/new"),
            Variant::Primary,
            Size::Normal,
            Some(Glyph::Plus),
            "Create topic",
        )
    };
    html! {
        (head(v, base, "Nothing has been created or written yet."))
        (empty(
            Glyph::Topic, "d-data", Some("teaching"),
            html! { "This broker holds no topics" },
            html! {
                "A topic is a named log of records, split into partitions. Create one here, or let a client make it: "
                span class="mono" { "--auto-create-topics" } " is on, so the first request for a name that does not exist creates it with "
                span class="mono" { "--default-partitions" } " (1)."
            },
            create(),
        ))
        (sec(32, "Or from your app", Some(html! { "Writing to a topic that does not exist creates it, and the console shows it here as soon as the request lands." }), html! {
            pre class="code" data-extra="copy-as" { span class="c" { "# kcat, from the machine the broker runs on" } "\n" (format!("echo '{{\"order_id\":\"ord-88412\"}}' | kcat -P -b {listen} -t orders -k ord-88412")) }
            div class="row" style="margin-top:12px" {
                (client::copy_button("btn sm", &format!("echo '{{\"order_id\":\"ord-88412\"}}' | kcat -P -b {listen} -t orders -k ord-88412"), html! { (glyph(Glyph::Copy, 14, "")) "Copy" }))
                span class="dim" { "To " span class="mono" { (listen) } ", the listener" }
            }
        }))
    }
}

/// The list's place when no topic name contains the filter: the toolbar above it stays exactly as it was.
fn nothing_matches(v: &TopicsView, base: &Base) -> Markup {
    let clear = base.url("/topics");
    empty(
        Glyph::Search,
        "",
        None,
        html! { "No topic name contains " span class="mono" { (v.filter) } },
        html! { "The filter looks at the " (v.total) " topic names; internal topics are hidden. Names are matched as typed, so " span class="mono" { "pick-lists" } " will not match " span class="mono" { "pick_lists" } "." },
        html! { div class="row" {
            (inplace::link(LIST_VIEW, "btn", &clear, None, html! { "Clear filter" }))
            a class="btn pri" href=(format!("{}?name={}", base.url("/topics/new"), segment(&v.filter))) { (glyph(Glyph::Plus, 16, "")) "Create " span class="mono" { (v.filter) } }
        } },
    )
}

fn steady(v: &TopicsView, base: &Base, query: &str) -> Markup {
    html! {
        (head(v, base, "Where records live. Every topic is a log: open one to read it, produce to it, or see what the broker did to it."))
        (inplace::region(LIST_VIEW, html! {
            // About the broker, not the filter: it is there whatever is typed, so typing moves nothing; it is in the region because
            // it says whether the internal topic is hidden, which a chip changes.
            (stats_fragment(v, base, query))
            (toolbar(v, base))
            @if v.state() == TopicsState::Filtered {
                (nothing_matches(v, base))
            } @else {
                (list_fragment(v, base, query))
                (internal_note(v, base))
            }
        }))
    }
}

/// The page, in the state the data calls for. `query` is the address's query part (`?q=...`) the fragments repeat.
#[must_use]
pub fn list_page(v: &TopicsView, base: &Base, listen: &str, query: &str) -> Markup {
    match v.state() {
        TopicsState::None => none(v, base, listen),
        TopicsState::List | TopicsState::Filtered => steady(v, base, query),
    }
}
