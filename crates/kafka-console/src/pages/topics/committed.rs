//! `topic-readcommitted`: what each isolation level gets, where each reader stops, and who is held back.

use maud::{Markup, html};

use super::common::{Acts, topic_head, topic_tabs, topic_url};
use super::messages::{messages_url, reading};
use super::table::records;
use crate::base::Base;
use crate::format::number;
use crate::model::{Hold, MessageQuery, MessagesView, PartitionRow};
use crate::ui::glyph::Glyph;
use crate::ui::kafka::{Mark, ruler, sec};
use crate::ui::notice::{Tone, callout};

fn toolbar(v: &MessagesView, base: &Base) -> Markup {
    html! {
        div class="toolbar" {
            (reading(v, base, true))
            span class="sp" {}
            span class="dim" { "The console reads read_uncommitted by default, so you can see everything" }
        }
    }
}

fn banner(v: &MessagesView) -> Markup {
    let held: Vec<&PartitionRow> = v.head.parts.iter().filter(|p| p.held() > 0).collect();
    if held.is_empty() {
        return html! {};
    }
    let total: i64 = held.iter().map(|p| p.held()).sum();
    let per: Vec<String> = held
        .iter()
        .map(|p| format!("{} on {}-{}", p.held(), v.head.name, p.partition))
        .collect();
    let ids: Vec<&str> = v.head.txns.iter().map(|t| t.id.as_str()).collect();
    html! {
        div style="margin-bottom:24px" { (callout(Tone::Warn, Glyph::Warn, html! {
            b { (total) " record" @if total != 1 { "s are" } @else { " is" } " held back from read_committed consumers" }
            ": " (crate::words::list_and(&per))
            @if !ids.is_empty() { ", all in " @if ids.len() == 1 { "transaction " } @else { "transactions " } (crate::words::list_and(&ids)) }
            ". Until it commits or aborts, a consumer reading read_committed stops at the last stable offset and sees nothing past it."
        })) }
    }
}

fn zoomed(p: &PartitionRow, topic: &str) -> Markup {
    let from = (p.end - 14).max(p.start);
    let span = (p.end - from).max(1) as f64;
    let at = |o: i64| (o - from) as f64 * 100.0 / span;
    let marks = [
        Mark {
            name: "LSO",
            class: "lso",
            offset: p.last_stable,
            at: at(p.last_stable),
        },
        Mark {
            name: "HW",
            class: "hw",
            offset: p.end,
            at: 100.0,
        },
    ];
    html! {
        div class="card" {
            h2 { (topic) "-" (p.partition) }
            p class="lede" { "Where each reader stops, zoomed to the last " (p.end - from) " offsets." }
            (ruler(from, p.last_stable, p.end, &marks))
        }
    }
}

fn levels(p: &PartitionRow) -> Markup {
    html! {
        div class="card" {
            h2 { "What each isolation level gets" }
            p class="lede" { "The same partition, the same Fetch, one field apart." }
            dl class="kv sm" {
                dt { "read_uncommitted" } dd { "offsets up to " span class="mono" { (number(p.end)) } span class="sub" { "every record, including those of the open transaction" } }
                dt { "read_committed" } dd { "offsets up to " span class="mono" { (number(p.last_stable)) } span class="sub" { "stops before the first open record, " (p.held()) " behind" } }
                dt { "After a commit" } dd { "the LSO moves to " (number(p.end)) " at once" span class="sub" { "the commit marker ends the hold" } }
                dt { "After an abort" } dd { "the " (p.held()) " records are skipped" span class="sub" { "the aborted range is listed in the Fetch answer" } }
            }
        }
    }
}

fn holds(v: &MessagesView, base: &Base) -> Markup {
    let row = |h: &Hold| {
        let held = (h.high_watermark - h.last_stable).max(0);
        html! { tr {
            td class="mono" { (v.head.name) "-" (h.partition) }
            td class="mono r" { (number(h.high_watermark)) }
            td class="mono r" { (number(h.last_stable)) }
            td class="mono r" { @if held > 0 { span class="st warn" { (held) } } @else { span class="st ok" { "0" } } }
            td { @match &h.txn { Some((id, first)) => { (id) " began at offset " (number(*first)) }, None => { span class="dim" { "no open transaction" } } } }
        } }
    };
    sec(
        40,
        "Who this holds back",
        Some(
            html! { "Per partition, where a read_committed consumer stops. A group reading read_committed has its lag counted up to the last stable offset." },
        ),
        html! {
            table class="t" {
                thead { tr { th scope="col" { "Partition" } th scope="col" class="r" { "High watermark" } th scope="col" class="r" { "Last stable offset" } th scope="col" class="r" { "Held back" } th scope="col" { "Why" } } }
                tbody { @for h in &v.holds { (row(h)) } }
            }
            p class="help" style="margin-top:16px" {
                @if let Some(t) = v.head.txns.first() {
                "The transaction times out at " (crate::words::timeout(t.timeout_ms)) ": the coordinator then aborts it and fences producer " (t.producer_id) ". "
            } @else { "A transaction that times out is aborted by the coordinator and its producer is fenced. " }
                a class="lnk" href=(format!("{}/producers", topic_url(base, &v.head.name))) { "What it holds back" }
            }
        },
    )
}

fn reader_sees(v: &MessagesView, base: &Base) -> Markup {
    let plain = messages_url(base, &v.head.name, &MessageQuery::default());
    html! {
        div class="sec" {
            h2 { "What a read_committed reader sees" }
            (records(&v.rows, &v.head.name, &v.query.to_query(), base))
            p class="help" style="margin-top:12px" {
                @match v.head.held() {
                    0 => { "Nothing is held back, so this is every record the table shows; " },
                    1 => { "The held record is not listed here; " },
                    n => { "The " (n) " held records are not listed here; " },
                }
                a class="lnk" href=(plain) { "read_uncommitted shows them" } ", each marked Above LSO."
            }
        }
    }
}

/// The page.
#[must_use]
pub(super) fn committed_page(v: &MessagesView, base: &Base, _query: &str) -> Markup {
    let focus = v
        .head
        .parts
        .iter()
        .max_by_key(|p| (p.held(), -i64::from(p.partition)))
        .or_else(|| v.head.parts.first());
    html! {
        (topic_head(&v.head, base, Acts::Full))
        (topic_tabs(&v.head, base))
        (toolbar(v, base))
        (banner(v))
        @if let Some(p) = focus { div class="g2" { (zoomed(p, &v.head.name)) (levels(p)) } }
        (holds(v, base))
        (reader_sees(v, base))
    }
}
