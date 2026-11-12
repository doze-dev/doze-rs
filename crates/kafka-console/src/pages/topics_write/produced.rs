//! The state after a record is sent (board `topic-produced`): the banner, the topic as it is now, the record in its
//! table, the answer, and whether a read_committed consumer can see it.

use maud::{Markup, html};

use super::topic_url;
use crate::base::Base;
use crate::format::{bytes, number};
use crate::model::Produced;
use crate::model::write::RecordRow;
use crate::phrases::{acked, clock_ms, head_sentence};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_extra::{Extra, glyph_extra};
use crate::ui::layout::card;
use crate::ui::write::{apicall, spans};

fn tabs(p: &Produced, base: &Base) -> Markup {
    let t = &p.topic;
    html! {
        div class="tabs" {
            a class="tab on" href=(topic_url(base, t, "")) { "Messages" }
            a class="tab" href=(topic_url(base, t, "/log")) { "Log" }
            a class="tab" href=(topic_url(base, t, "/config")) { "Config" }
            a class="tab" href=(base.url("/groups")) { "Consumers" }
            a class="tab" href=(topic_url(base, t, "/producers")) { "Producers" }
        }
    }
}

fn header(p: &Produced, base: &Base) -> Markup {
    let h = &p.head;
    html! {
        div class="page-h" {
            div {
                div class="title-row" {
                    h1 class="mono" { (h.name) }
                    span class="pill mono" { (h.cleanup) }
                    @if !h.transactions.is_empty() { span class="st warn" { "Transaction open" } }
                }
                p class="sent" data-extra="live-sentence" style="margin:10px 0 0;max-width:760px" { (spans(&head_sentence(h))) }
            }
            div class="acts" {
                a class="btn" href=(topic_url(base, &h.name, "/partitions")) { (glyph_extra(Extra::List, 16)) "Partitions" }
                a class="btn pri" href=(topic_url(base, &h.name, "/produce")) { (glyph_extra(Extra::Send, 16)) "Produce" }
                a class="btn danger" href=(topic_url(base, &h.name, "/delete")) { (glyph_extra(Extra::Trash, 16)) "Delete topic" }
            }
        }
    }
}

fn visible(p: &Produced) -> Markup {
    let v = &p.visibility;
    let (lead, rest) = if p.row.committed {
        (
            "Yes, to both isolation levels.",
            format!(
                "{topic}-{part} has no open transaction below this record: its last stable offset is {lso} and its high watermark {hw}. A consumer reading read_committed gets offset {off} as soon as it asks.",
                topic = p.topic,
                part = p.partition,
                lso = number(v.last_stable),
                hw = number(v.high_watermark),
                off = number(p.offset)
            ),
        )
    } else {
        (
            "To read_uncommitted now; to read_committed when the transaction ends.",
            format!(
                "{topic}-{part} has an open transaction: its last stable offset is {lso}, below this record at {off}. A consumer reading read_committed does not get the record until the transaction commits or aborts; the high watermark is {hw}.",
                topic = p.topic,
                part = p.partition,
                lso = number(v.last_stable),
                hw = number(v.high_watermark),
                off = number(p.offset)
            ),
        )
    };
    card(html! { h2 { "Is it visible?" } p class="lede" { (lead) } p style="margin:0" { (rest) } })
}

fn row(r: &RecordRow, hot: bool) -> Markup {
    let shown: String = r.value.chars().take(46).collect();
    let cut = if r.value.chars().count() > 46 {
        "…"
    } else {
        ""
    };
    html! {
        tr class=[hot.then_some("hot")] {
            td class="mono" { (r.partition) }
            td class="mono" { (number(r.offset)) }
            td class="mono dim" { (clock_ms(r.timestamp_ms)) }
            td class="mono" { (r.key) }
            td class="mono" style="font-size:12.5px" { (shown) (cut) }
            td class="mono r" { (bytes(r.size)) }
            td { @if r.committed { span class="st ok" { "Committed" } } @else { span class="st warn" { "Above LSO" } } }
        }
    }
}

/// The page.
#[must_use]
pub fn produced(p: &Produced, base: &Base) -> Markup {
    html! {
        div class="banner" style="margin-top:24px" {
            (glyph(Glyph::Check, 18, ""))
            span { b { "Sent to " (p.topic) "-" (p.partition) " at offset " (number(p.offset)) "." } " Timestamp " (clock_ms(p.timestamp_ms)) ", " (acked(p.acks)) "." }
        }
        (header(p, base))
        (tabs(p, base))
        div {
            table class="t" {
                thead { tr {
                    th scope="col" { "Partition" } th scope="col" { "Offset" } th scope="col" { "Timestamp" } th scope="col" { "Key" }
                    th scope="col" { "Value" } th scope="col" class="r" { "Value size" } th scope="col" { "For read_committed" }
                } }
                tbody { (row(&p.row, true)) @for o in &p.others { (row(o, false)) } }
            }
            div class="pager" { span { @if p.others.is_empty() { "The record just sent · " (number(p.head.records)) " in the topic" } @else { "Newest first · " (p.others.len() + 1) " of " (number(p.head.records)) } } }
        }
        div class="g2" style="margin-top:32px" {
            div { (apicall("The answer", &p.answer, Some("The log chose the offset; the producer only supplies the record. -1 means the topic keeps the producer's time (CreateTime)."))) }
            (visible(p))
        }
    }
}
