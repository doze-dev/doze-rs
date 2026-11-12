//! The Messages page of a topic: `topic-detail`, `topic-messages-filtered`, `topic-messages-empty` and `topic-readcommitted`
//! (the offline state is `offline.rs`). The state is chosen by the data and the filters, not by the address.

use maud::{Markup, html};

use super::common::{Acts, spans, topic_head, topic_tabs, topic_url, txn_callout};
use super::table::records;
use crate::base::Base;
use crate::format::number;
use crate::model::{MessageQuery, MessagesView, Reading};
use crate::ui::button::{Size, State, Variant, opener, state};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::kafka::{apicall, empty, filter_box, pager, sec};
use crate::words::reach_sentence;

/// The fragment that holds the table and what is said about it.
pub const MESSAGES_FRAGMENT: &str = "topic-messages";

/// `/topics/orders/messages?...`.
pub(super) fn messages_url(base: &Base, topic: &str, q: &MessageQuery) -> String {
    format!("{}/messages{}", topic_url(base, topic), q.to_query())
}

fn notes(v: &MessagesView) -> Markup {
    html! { @for n in &v.notes { p class="help" role="status" style="margin:0 0 12px" { (n) } } }
}

fn older(v: &MessagesView, base: &Base) -> Markup {
    let q = MessageQuery {
        limit: (v.query.limit * 2).min(crate::model::MOST_LIMIT),
        ..v.query.clone()
    };
    html! {
        @if v.reach.more && v.query.limit < crate::model::MOST_LIMIT {
            a class="btn sm" data-ui="page" href=(messages_url(base, &v.head.name, &q)) { "Older" }
        }
    }
}

fn line(v: &MessagesView) -> String {
    if v.query.filtered() {
        return format!(
            "Newest first · {}",
            crate::format::count(v.reach.returned, "record", "records")
        );
    }
    format!(
        "Newest first · {} of {}",
        v.reach.returned,
        number(v.head.records())
    )
}

/// The table region as a fragment: what is said about the read, the table, and the line under it.
#[must_use]
pub fn messages_fragment(v: &MessagesView, base: &Base, page_query: &str) -> Markup {
    let url = base.url(&format!(
        "/fragments/{MESSAGES_FRAGMENT}?topic={}&{}",
        crate::base::segment(&v.head.name),
        page_query.trim_start_matches('?')
    ));
    client::fragment(
        MESSAGES_FRAGMENT,
        &url,
        html! {
            (notes(v))
            @if v.query.filtered() {
                p class="sent" style="margin:0 0 16px;max-width:900px" { (spans(&reach_sentence(v))) }
            }
            @if v.rows.is_empty() && v.query.filtered() {
                p class="help" { "Nothing to list. Clear the filters to see the newest records." }
            } @else {
                (records(&v.rows, &v.head.name, &v.query.to_query(), base))
                @if !v.query.filtered() { (pager(&line(v), older(v, base))) }
            }
        },
    )
}

/// The segmented control of partitions, as buttons of a plain GET form.
fn partition_seg(v: &MessagesView) -> Markup {
    html! {
        div class="seg" aria-label="Partition" {
            button class={@if v.query.partition.is_none() { "on" }} type="submit" name="partition" value="" data-ui="select" { "All partitions" }
            @for p in &v.head.parts {
                button class={@if v.query.partition == Some(p.partition) { "on" }} type="submit" name="partition" value=(p.partition) data-ui="select" { (p.partition) }
            }
        }
    }
}

/// The two ways of reading, the one showing as a chip and the other as a link.
pub(super) fn reading(v: &MessagesView, base: &Base, other: bool) -> Markup {
    let url = |r: Reading| {
        let q = MessageQuery {
            reading: r,
            ..v.query.clone()
        };
        messages_url(base, &v.head.name, &q)
    };
    let (on, off) = match v.query.reading {
        Reading::Uncommitted => (Reading::Uncommitted, Reading::Committed),
        Reading::Committed => (Reading::Committed, Reading::Uncommitted),
    };
    html! {
        span class="dim" { "Reading as" }
        @if v.query.reading == Reading::Committed {
            a class="fchip" href=(url(off)) { (off.word()) }
            span class="fchip on" { (on.word()) }
        } @else {
            span class="fchip on" { (on.word()) }
            @if other { a class="fchip" href=(url(off)) { (off.word()) } }
        }
    }
}

fn from_held(v: &MessagesView, base: &Base) -> Markup {
    let Some(p) = v.head.parts.iter().find(|p| p.held() > 0) else {
        return html! {};
    };
    let q = MessageQuery {
        partition: Some(p.partition),
        offset: Some(p.last_stable),
        ..MessageQuery::default()
    };
    html! {
        a class="btn sm" href=(messages_url(base, &v.head.name, &q)) data-op="Fetch" {
            (glyph(Glyph::Filter, 14, "")) "Show " (v.head.name) "-" (p.partition) " from " (number(p.last_stable))
        }
    }
}

fn form(v: &MessagesView, base: &Base, inner: Markup) -> Markup {
    html! {
        form class="toolbar" method="get" action=(format!("{}/messages", topic_url(base, &v.head.name))) {
            // The button Enter presses in the find box: not a control of the page, so it is out of the board's reckoning.
            input type="submit" value="Show" aria-label="Show records" hidden data-scaffold;
            @if v.query.reading == Reading::Committed { input type="hidden" name="isolation" value="read_committed"; }
            (inner)
        }
    }
}

pub(super) fn detail_toolbar(v: &MessagesView, base: &Base) -> Markup {
    form(
        v,
        base,
        html! {
            (partition_seg(v))
            (filter_box("find", "Key, or offset", "Find by key or offset", ""))
            (from_held(v, base))
            span class="sp" {}
            (reading(v, base, true))
        },
    )
}

fn chip(on: bool, label: &str, value: &str) -> Markup {
    html! { span class={"fchip" @if on { " on" }} { (label) @if on { " " (value) } } }
}

fn filtered_toolbar(v: &MessagesView, base: &Base) -> Markup {
    let q = &v.query;
    let clear = messages_url(
        base,
        &v.head.name,
        &MessageQuery {
            reading: q.reading,
            ..MessageQuery::default()
        },
    );
    form(
        v,
        base,
        html! {
            (partition_seg(v))
            (chip(q.partition.is_some(), "Partition", &q.partition.map(|p| p.to_string()).unwrap_or_default()))
            @if q.partition.is_some() { input type="hidden" name="partition" value=(q.partition.unwrap_or(0)); }
            (chip(q.offset.is_some(), "From offset", &q.offset.map(number).unwrap_or_default()))
            label class={"fchip" @if q.time.is_some() { " on" }} { "Time " input type="text" name="time" value=(q.time.as_ref().map_or("", |t| t.1.as_str())) placeholder="2026-11-09 22:13" aria-label="From time (UTC)" size="16"; }
            label class={"fchip" @if q.key.is_some() { " on" }} { "Key " input type="text" name="key" value=(q.key.as_deref().unwrap_or("")) placeholder="exact key" aria-label="Key" size="14"; }
            @if let Some(o) = q.offset { input type="hidden" name="offset" value=(o); }
            a class="btn sm" data-ui="clear" href=(clear) { (glyph(Glyph::X, 14, "")) "Clear filters" }
            span class="sp" {}
            (reading(v, base, false))
        },
    )
}

fn call_preview(v: &MessagesView) -> Markup {
    let q = &v.query;
    let partition = q.partition.unwrap_or(0);
    let from = q.offset.unwrap_or(0);
    let iso = if q.reading == Reading::Committed {
        "1 (read_committed)"
    } else {
        "0 (read_uncommitted)"
    };
    apicall(
        "The call",
        html! {
            span class="k" { "Fetch" } " v" (v.head.calls.fetch) "\n  isolation_level: " (iso) "\n  topics[0]\n    topic: " span class="s" { (v.head.name) }
            "\n    partitions[0]\n      partition: " (partition) "\n      fetch_offset: " (from) "\n      partition_max_bytes: " (crate::backend::requests::TABLE_BYTES)
        },
        Some(
            html! { "A key search reads the partition backwards from its end, a thousand offsets at a time, until enough records match or 8 MiB have been looked at; a time goes to the first record at or after it; an offset goes straight to the batch that holds it." },
        ),
    )
}

fn why_offset() -> Markup {
    html! {
        div class="card" {
            h2 { "Why filter by offset" }
            p class="lede" { "Ask for exactly the records you want to look at." }
            p style="margin:0" { "A Fetch names a partition and an offset and gets whole batches from the one that holds it, up to the byte limit. Asking from the last stable offset returns the first record of the open transaction onward. A key search has no such shortcut: it reads the partition in windows from its end." }
        }
    }
}

fn detail(v: &MessagesView, base: &Base, query: &str) -> Markup {
    html! {
        (topic_head(&v.head, base, Acts::Full))
        (topic_tabs(&v.head, base))
        @if v.query.reading == Reading::Uncommitted { (txn_callout(&v.head, base)) }
        (detail_toolbar(v, base))
        (messages_fragment(v, base, query))
        div class="row wrap" style="margin-top:20px;gap:24px" {
            span class="row" { (state(State::Ok, &format!("{} of {} partitions online", v.head.parts.len() - v.head.offline().len(), v.head.parts.len()))) }
        }
    }
}

fn filtered(v: &MessagesView, base: &Base, query: &str) -> Markup {
    html! {
        (topic_head(&v.head, base, Acts::Full))
        (topic_tabs(&v.head, base))
        (filtered_toolbar(v, base))
        (messages_fragment(v, base, query))
        div class="g2" style="margin-top:32px" { div { (call_preview(v)) } (why_offset()) }
    }
}

fn empty_topic(v: &MessagesView, base: &Base, listen: &str) -> Markup {
    let produce = opener(
        &format!("{}/produce", topic_url(base, &v.head.name)),
        Variant::Primary,
        Size::Normal,
        Some(Glyph::Send),
        "Produce a record",
    );
    let cmd = format!(
        "echo '{{\"order_id\":\"ord-88412\",\"error\":\"payment declined\"}}' \\\n  | kcat -P -b {listen} -t {} -k ord-88412",
        v.head.name
    );
    html! {
        (topic_head(&v.head, base, Acts::Empty))
        (topic_tabs(&v.head, base))
        (empty(
            Glyph::Queue, "d-msg", Some("teaching"),
            html! { "No records yet" },
            html! { "Nothing has been written to this topic, so there is nothing to read, and no group or producer has touched it. The first record lands at offset 0 of partition 0." },
            produce,
        ))
        (sec(32, "Or from your app", Some(html! { "The first record lands at offset 0 of partition 0." }), html! {
            pre class="code" data-extra="copy-as" { (cmd) }
            div class="row" style="margin-top:12px" { (client::copy_button("btn sm", &cmd, html! { (glyph(Glyph::Copy, 14, "")) "Copy" })) }
        }))
    }
}

/// The page for a topic's Messages, in the state its data and filters call for.
#[must_use]
pub fn messages_page(v: &MessagesView, base: &Base, listen: &str, query: &str) -> Markup {
    if v.head.is_empty() && !v.query.filtered() && v.query.reading == Reading::Uncommitted {
        return empty_topic(v, base, listen);
    }
    if !v.head.offline().is_empty()
        && !v.query.filtered()
        && v.query.reading == Reading::Uncommitted
    {
        return super::offline::offline_page(&v.head, base);
    }
    if v.query.reading == Reading::Committed {
        return super::committed::committed_page(v, base, query);
    }
    if v.query.filtered() {
        return filtered(v, base, query);
    }
    detail(v, base, query)
}
