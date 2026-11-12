//! The topics list in its three states: `topics-list`, `topics-none` and `topics-filtered`.

use maud::{Markup, html};

use super::common::spans;
use crate::base::{Base, segment};
use crate::format::{ago, bytes};
use crate::model::{Cleanup, TopicRow, TopicsState, TopicsView};
use crate::ui::button::{Size, State, Variant, link, state};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::kafka::{Tab, empty, filter_box, pager, sec, tabs};
use crate::ui::layout::page_header;
use crate::ui::notice::{Tone, callout};
use crate::words::{list_sentence, retention};

/// The fragments of the list: the sentence and the table.
pub const LIST_FRAGMENT: &str = "topics-table";
pub const LIST_SENTENCE: &str = "topics-sentence";

fn cleanup_tag(c: Cleanup) -> Markup {
    let word = super::common::policy_word(c);
    html! { span class="tag" { (word) } }
}

fn sub_line(r: &TopicRow) -> Option<String> {
    if r.internal {
        return Some("read only · the group coordinator's name for where commits live".to_owned());
    }
    if r.empty {
        return Some("empty: no records yet".to_owned());
    }
    let mut parts = Vec::new();
    if let Some(n) = r.segments.filter(|n| *n > 1) {
        parts.push(format!("{n} segments"));
    }
    if let Some(ms) = r.retention_ms {
        parts.push(format!("retention {}", retention(ms)));
    }
    if !r.read_by.is_empty() {
        parts.push(format!("read by {}", r.read_by.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn row(r: &TopicRow, v: &TopicsView, base: &Base) -> Markup {
    let href = base.url(&format!("/topics/{}", segment(&r.name)));
    let sub = sub_line(r);
    html! {
        tr {
            td class="name" {
                @if r.internal {
                    span class="mono" { (r.name) } " " span class="tag" { "internal" }
                } @else {
                    a class="mono" href=(href) { (r.name) }
                    @if r.offline > 0 { " " (state(State::Bad, "Offline")) }
                    @if r.txn_open { " " (state(State::Warn, "Transaction open")) }
                }
                @if let Some(s) = sub { span class="sub" { (s) } }
            }
            td class="mono r" { (r.partitions) }
            td { (cleanup_tag(r.cleanup)) }
            @if r.internal {
                td class="mono r dim" { "not a log" }
                td class="mono r dim" { "none" }
            } @else {
                td class="mono r" { (r.bytes.map_or_else(|| "unknown".to_owned(), bytes)) }
                td class="mono r" data-vary {
                    @match r.last_write_ms {
                        Some(ms) if !r.empty => (ago(v.now_ms - ms)),
                        _ => "never",
                    }
                }
            }
        }
    }
}

fn table(v: &TopicsView, base: &Base) -> Markup {
    html! {
        table class="t" {
            thead { tr {
                th scope="col" { "Name" } th scope="col" class="r" { "Partitions" } th scope="col" { "Cleanup policy" }
                th scope="col" class="r" { "Size on disk" } th scope="col" class="r" { "Last write" }
            } }
            tbody { @for r in &v.rows { (row(r, v, base)) } }
        }
    }
}

fn shown(v: &TopicsView) -> String {
    let n = v.rows.iter().filter(|r| !r.internal).count();
    let of = if v.filter.is_empty() { v.total } else { n };
    if v.truncated {
        format!(
            "First {n} of {} topics, by name",
            crate::format::number(i64::try_from(v.total).unwrap_or(0))
        )
    } else {
        format!("{n} of {of} topics")
    }
}

/// The sentence under the title, as a fragment.
#[must_use]
pub fn sentence_fragment(v: &TopicsView, base: &Base, query: &str) -> Markup {
    client::fragment(
        LIST_SENTENCE,
        &base.url(&format!("/fragments/{LIST_SENTENCE}{query}")),
        html! { p class="sent" data-extra="live-sentence" style="margin:-8px 0 20px" { (spans(&list_sentence(v))) } },
    )
}

/// The table and the line under it, as a fragment.
#[must_use]
pub fn list_fragment(v: &TopicsView, base: &Base, query: &str) -> Markup {
    client::fragment(
        LIST_FRAGMENT,
        &base.url(&format!("/fragments/{LIST_FRAGMENT}{query}")),
        html! { (table(v, base)) (pager(&shown(v), html! {})) },
    )
}

fn toolbar(v: &TopicsView, base: &Base) -> Markup {
    html! {
        form class="toolbar" method="get" action=(base.url("/topics")) {
            // The button Enter presses in the filter box: not a control of the page, so it is out of the board's reckoning.
            input type="submit" value="Filter" aria-label="Filter topics" hidden data-scaffold;
            (filter_box("q", "Filter topics by name", "Filter topics by name", &v.filter))
            button class={"fchip" @if v.show_internal { " on" }} type="submit" name="internal" value=(if v.show_internal { "0" } else { "1" }) data-ui="toggle" aria-pressed=(v.show_internal) {
                span class="toggle" {} "Show internal"
            }
            span class="sp" {}
            button class="btn sm" type="submit" data-ui="refresh" { (glyph(Glyph::Refresh, 14, "")) "Refresh" }
            @if v.show_internal { input type="hidden" name="internal" value="1"; }
        }
    }
}

fn internal_note(v: &TopicsView, base: &Base) -> Markup {
    let row = TopicRow::internal();
    html! {
        div style="margin-top:24px" {
            (callout(Tone::Plain, Glyph::Info, html! {
                div style="flex:1" {
                    b { @if v.show_internal { "The internal topic is shown." } @else { "One internal topic is hidden." } }
                    " Turn on " b { "Show internal" } " and this row joins the list:"
                    table class="t" style="margin-top:12px;background:var(--bg)" { tbody { (row_internal(&row)) } }
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

fn row_internal(r: &TopicRow) -> Markup {
    html! {
        tr {
            td class="name" { span class="mono" { (r.name) } " " span class="tag" { "internal" } span class="sub" { "read only · the group coordinator's name for where commits live" } }
            td class="mono r" { (r.partitions) }
            td { (cleanup_tag(r.cleanup)) }
            td class="mono r dim" { "not a log" }
            td class="mono r dim" { "none" }
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

fn filtered(v: &TopicsView, base: &Base) -> Markup {
    let clear = base.url("/topics");
    html! {
        (head(v, base, "Where records live. Every topic is a log: open one to read it, produce to it, or see what the broker did to it."))
        div class="toolbar" {
            (filter_box("q", "Filter topics by name", "Filter topics by name", &v.filter))
            span class="fchip on" { "Name contains " (v.filter) }
            a class="btn sm" href=(clear) data-ui="clear" { (glyph(Glyph::X, 14, "")) "Clear filter" }
        }
        (empty(
            Glyph::Search, "", None,
            html! { "No topic name contains " span class="mono" { (v.filter) } },
            html! { "The filter looks at the " (v.total) " topic names; internal topics are hidden. Names are matched as typed, so " span class="mono" { "pick-lists" } " will not match " span class="mono" { "pick_lists" } "." },
            html! { div class="row" {
                a class="btn" href=(clear) data-ui="clear" { "Clear filter" }
                a class="btn pri" href=(format!("{}?name={}", base.url("/topics/new"), segment(&v.filter))) { (glyph(Glyph::Plus, 16, "")) "Create " span class="mono" { (v.filter) } }
            } },
        ))
    }
}

fn steady(v: &TopicsView, base: &Base, query: &str) -> Markup {
    html! {
        (head(v, base, "Where records live. Every topic is a log: open one to read it, produce to it, or see what the broker did to it."))
        @if v.filter.is_empty() { (sentence_fragment(v, base, query)) }
        (toolbar(v, base))
        (list_fragment(v, base, query))
        (internal_note(v, base))
    }
}

/// The page, in the state the data calls for. `query` is the address's query part (`?q=...`) the fragments repeat.
#[must_use]
pub fn list_page(v: &TopicsView, base: &Base, listen: &str, query: &str) -> Markup {
    match v.state() {
        TopicsState::None => none(v, base, listen),
        TopicsState::Filtered => filtered(v, base),
        TopicsState::List => steady(v, base, query),
    }
}
