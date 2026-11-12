//! The Messages page of a topic: `topic-detail`, `topic-messages-filtered`, `topic-messages-empty` and `topic-readcommitted`
//! (the offline state is `offline.rs`). The state is chosen by the data and the filters, not by the address.

use maud::{Markup, html};

use super::bar::bar;
use super::common::{TopicTab, spans, topic_frame, topic_url, txn_callout};
use super::table::records;
use crate::base::Base;
use crate::format::number;
use crate::model::{MessageQuery, MessagesView, Reading};
use crate::ui::button::{Size, State, Variant, opener, state};
use crate::ui::client;
use crate::ui::disclosure::{Look, disclosure};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::kafka::{apicall, empty, pager, sec};
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
            (isolation_note(v))
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

/// What is said about the read at the top of the data: for `read_committed`, where each reader stops and what is held back.
fn isolation_note(v: &MessagesView) -> Markup {
    if v.query.reading != Reading::Committed {
        return html! {};
    }
    html! {
        p class="help" role="status" style="margin:0 0 12px" {
            @match v.head.held() {
                0 => { "Reading as read_committed. Nothing is held back, so this is every record the table shows." },
                1 => { "Reading as read_committed. One record is held back (it is above the last stable offset) and is not listed; read_uncommitted shows it, marked Above LSO." },
                n => { "Reading as read_committed. " (n) " records are held back (they are above the last stable offset) and are not listed; read_uncommitted shows them, each marked Above LSO." },
            }
        }
    }
}

/// The explainers under the table, in one section that is open when the page is reading `read_committed` (it is the part
/// of the page that explains that level) and closed otherwise: opening or closing it moves only what is below it.
fn explainers(v: &MessagesView, base: &Base) -> Markup {
    let committed = v.query.reading == Reading::Committed;
    disclosure(
        Look::Plain,
        Some("topic-explained"),
        committed,
        html! { "How this read works" },
        html! {
            @if committed { (super::committed::more(v, base)) }
            div class="g2" style=(if committed { "margin-top:24px" } else { "" }) { div { (call_preview(v)) } (why_offset()) }
        },
    )
}

fn detail(v: &MessagesView, base: &Base, query: &str) -> Markup {
    html! {
        (topic_frame(&v.head, base, TopicTab::Messages))
        // Whenever the topic has an open transaction, whatever is being filtered or read: the structure does not depend on the state.
        (txn_callout(&v.head, base))
        (bar(v, base))
        (messages_fragment(v, base, query))
        div class="row wrap" style="margin-top:20px;gap:24px" {
            span class="row" { (state(State::Ok, &format!("{} of {} partitions online", v.head.parts.len() - v.head.offline().len(), v.head.parts.len()))) }
        }
        div style="margin-top:24px" { (explainers(v, base)) }
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
        (topic_frame(&v.head, base, TopicTab::Messages))
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
    detail(v, base, query)
}
