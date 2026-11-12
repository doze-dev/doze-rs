//! One record, completely (`topic-record` and `topic-record-binary`): a drawer on the boards, a page of its own here so the
//! address works with no script.

use maud::{Markup, html};

use super::common::topic_url;
use crate::base::Base;
use crate::format::{bytes, clock_ms, number, span_ms};
use crate::model::{Kind, RecordView, Shown, Standing};
use crate::ui::button::{State, state};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::kafka::{field, headers};
use crate::ui::notice::{Tone, callout};

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Null => "null",
        Kind::Empty => "empty",
        Kind::Text => "text",
        Kind::Json => "JSON",
        Kind::Binary => "binary",
    }
}

fn note(s: &Shown) -> String {
    let size = if s.bytes == 1 { "1 byte".to_owned() } else { format!("{} bytes", number(i64::try_from(s.bytes).unwrap_or(i64::MAX))) };
    match s.kind {
        Kind::Null => "null".to_owned(),
        _ => format!("{size} · {}", kind_word(s.kind)),
    }
}

/// The block under a label: the text, the hex, or a sentence for nothing; and how much was cut.
fn block(s: &Shown, style: &str, id: &str) -> Markup {
    html! {
        @match s.kind {
            Kind::Null => p class="help" style="margin:0" { "No value: a deletion (a tombstone)." },
            Kind::Empty => p class="help" style="margin:0" { "Zero bytes." },
            _ => {
                pre class="code" style=(style) id=(id) { (s.text) }
                @if s.clipped {
                    p class="help" style="margin:8px 0 0" { "Shown up to " (bytes(s.text.len() as u64)) " of " (bytes(s.bytes)) "; the rest is not shown." }
                }
            }
        }
    }
}

fn standing_callout(v: &RecordView) -> Markup {
    let lso = number(v.last_stable);
    match v.standing {
        Standing::AboveLso => html! {
            div data-extra="record-view" style="margin-bottom:20px" { (callout(Tone::Warn, Glyph::Warn, html! {
                b { "At or above the last stable offset." }
                " The LSO of " (v.topic) "-" (v.partition) " is " (lso) @if v.transactional { " and this record is in a transaction that is still open" }
                ": a read_committed consumer cannot see it yet; a read_uncommitted one can."
            })) }
        },
        Standing::Aborted => html! {
            div data-extra="record-view" style="margin-bottom:20px" { (callout(Tone::Warn, Glyph::Warn, html! {
                b { "Its transaction was aborted." } " A read_uncommitted consumer reads it; a read_committed one skips it."
            })) }
        },
        Standing::Committed if v.value.kind == Kind::Binary => html! {
            div data-extra="record-view" style="margin-bottom:20px" { (callout(Tone::Plain, Glyph::Info, html! {
                b { "The value is not text." } " " (number(i64::try_from(v.value.bytes).unwrap_or(0))) " bytes that are not valid UTF-8, shown as hex. Avro and Protobuf are decoded once schemas arrive (Phase 10); until then the bytes are all the broker can say."
            })) }
        },
        Standing::Committed => html! { div data-extra="record-view" {} },
    }
}

fn timestamp(v: &RecordView) -> Markup {
    html! {
        span data-vary { (clock_ms(v.timestamp_ms)) } " "
        span class="sub" { @if v.log_append_time { "LogAppendTime, set by the broker." } @else { "CreateTime, set by the producer." } }
    }
}

fn producer(v: &RecordView) -> Markup {
    match v.producer {
        Some((id, epoch, seq)) => html! { span class="mono" { (id) " · epoch " (epoch) @if seq >= 0 { " · sequence " (number(seq)) } } },
        None => html! { "none " span class="sub" { "a plain producer: no producer id, so no epoch or sequence" } },
    }
}

fn transaction(v: &RecordView) -> Markup {
    match &v.txn {
        None => html! { "none" },
        Some(t) => html! {
            @match &t.id { Some(id) => span class="mono" { (id) }, None => "unknown transaction" }
            " " @if let Some(s) = &t.state { (state(if s == "Ongoing" { State::Warn } else { State::Info }, s)) }
            @if let (Some(since), Some(timeout)) = (t.since_ms, t.timeout_ms) {
                span class="sub" { "since " (crate::format::clock(since)) ", timeout " (span_ms(timeout)) }
            } @else { span class="sub" { "the coordinator no longer has it open" } }
        },
    }
}

fn position(v: &RecordView) -> Markup {
    match v.standing {
        Standing::Committed => html! { (state(State::Ok, "Committed")) span class="sub" { "below the LSO of " (v.topic) "-" (v.partition) " (" (number(v.last_stable)) "): both isolation levels see it" } },
        Standing::AboveLso => html! { (state(State::Warn, "Above LSO")) span class="sub" { "at or above the LSO of " (v.topic) "-" (v.partition) " (" (number(v.last_stable)) ")" } },
        Standing::Aborted => html! { (state(State::Idle, "Aborted")) span class="sub" { "below the LSO, in a transaction that was aborted" } },
    }
}

/// The page for one record.
#[must_use]
pub fn record_page(v: &RecordView, base: &Base) -> Markup {
    let back = topic_url(base, &v.topic);
    let binary = v.value.kind == Kind::Binary;
    let hdrs: Vec<(String, String)> = v.headers.iter().map(|h| (h.key.clone(), if h.value.kind == Kind::Binary { h.value.preview.clone() } else { h.value.text.clone() })).collect();
    html! {
        div role="dialog" aria-labelledby="record-title" class="drawer-panel" style="position:relative;top:0;right:auto;bottom:auto;width:auto;max-width:640px;box-shadow:none;border:1px solid var(--line);border-radius:12px" {
            h2 class="mono" id="record-title" { (v.topic) "-" (v.partition) " · " (number(v.offset)) }
            p class="help" style="margin:0 0 16px" { "One record, completely." }
            (standing_callout(v))
            (field("Key", &note(&v.key), block(&v.key, "padding:10px 14px", "record-key")))
            (field("Value", &note(&v.value), block(&v.value, if binary { "padding:12px 14px;font-size:12px;line-height:20px" } else { "padding:12px 14px;font-size:12.5px;line-height:20px" }, "record-value")))
            (field("Headers", &(if hdrs.is_empty() { "none".to_owned() } else { hdrs.len().to_string() }), html! {
                @if hdrs.is_empty() { p class="help" style="margin:0" { "No headers were sent with this record." } } @else { (headers(&hdrs)) }
            }))
            dl class="kv sm" {
                dt { "Timestamp" } dd class="mono" { (timestamp(v)) }
                dt { "Codec" } dd { (v.codec) }
                dt { "Producer" } dd { (producer(v)) }
                dt { "Transaction" } dd { (transaction(v)) }
                @if v.txn.is_none() { dt { "Position" } dd { (position(v)) } }
            }
            div class="row wrap" style="margin-top:20px" {
                (client::copy_from_button("btn", "record-value", html! { (glyph(Glyph::Copy, 14, "")) @if binary { "Copy as hex" } @else { "Copy value" } }))
                a class="btn ghost" data-ui="close" href=(back) { "Close" }
            }
        }
    }
}

/// The page for a record that is not there: a topic or partition that does not exist, an offset outside the log, or a place
/// that compaction, a deletion or a transaction marker has emptied.
#[must_use]
pub fn no_record(topic: &str, partition: i32, offset: i64, base: &Base) -> Markup {
    html! {
        (crate::ui::layout::page_header(
            "No such record",
            html! { p class="sub" {
                "There is no record at offset " (number(offset)) " of " span class="mono" { (topic) "-" (partition) }
                ". The offset may be past the end of the log or before its start, or the record may have been removed by compaction, by a deletion, or be a transaction marker."
            } },
            html! {},
        ))
        div class="row" { a class="btn pri" href=(topic_url(base, topic)) { "Back to " (topic) } }
    }
}
