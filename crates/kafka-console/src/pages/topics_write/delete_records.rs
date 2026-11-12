//! Delete records: the dialog (board `topic-delete-records`) and the refused state (board `topic-delete-records-refused`).

use maud::{Markup, html};

use super::{field, hidden, submit, topic_url};
use crate::base::Base;
use crate::format::number;
use crate::model::{DeleteRecordsRefused, DeleteRecordsView, Span, Teach};
use crate::phrases::{positions, removed};
use crate::ui::dialog::{cancel, modal};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::card;
use crate::ui::notice::{Tone, callout};
use crate::ui::write::{apicall, field_error, refused, spans, teach, text_input};

fn teaching(v: &DeleteRecordsView) -> Teach {
    let hw = number(v.high_watermark);
    Teach {
        says: "the log start moves to this offset; earlier records are no longer served."
            .to_owned(),
        default: Vec::new(),
        range: vec![Span::text(format!(
            "0 up to the high watermark ({hw}). -1 means the high watermark: delete everything."
        ))],
        takes_effect: vec![Span::text(
            "at once, kept through a restart. Below or at the start changes nothing; past the high watermark is OFFSET_OUT_OF_RANGE.",
        )],
        here: vec![Span::text(
            "it cannot be undone, and the last stable offset never falls below the new start.",
        )],
    }
}

fn warning(v: &DeleteRecordsView) -> Markup {
    let to: i64 = v.offset.trim().parse().unwrap_or(v.log_start);
    let n = removed(v.log_start, v.high_watermark, to);
    let new_start = if to == -1 {
        v.high_watermark
    } else {
        to.min(v.high_watermark)
    };
    let part = format!("{}-{}", v.topic, v.partition);
    callout(
        Tone::Warn,
        Glyph::Warn,
        html! {
            b { "This removes " (number(n)) " records" }
            @if n > 0 {
                ": offsets " (number(v.log_start)) " to " (number(new_start - 1)) " of " (part) ". Offset " (number(new_start))
                " stays and becomes the log start. A consumer positioned below it gets OFFSET_OUT_OF_RANGE and follows its "
                span class="mono" { "auto.offset.reset" } "." (spans(&positions(&v.groups, new_start)))
            } @else {
                ". The log of " (part) " already starts at " (number(v.log_start)) " or later, so nothing changes."
            }
        },
    )
}

fn body(v: &DeleteRecordsView) -> Markup {
    let unit = format!(
        "log start now {} · end {}",
        number(v.log_start),
        number(v.high_watermark)
    );
    html! {
        div class="form" style="gap:18px" {
            (hidden(&[("partition", &v.partition.to_string())]))
            (field("o", html! { "Delete everything before offset" },
                text_input("o", "offset", &v.offset, true, Some(&unit), v.error.is_some()),
                html! { @if let Some(e) = &v.error { (field_error(e)) } (teach(&teaching(v))) }))
            (warning(v))
            div class="help" { "On a topic that only compacts this is refused (POLICY_VIOLATION)." }
            (apicall("The call", &v.call, None))
        }
    }
}

fn button_label(v: &DeleteRecordsView) -> String {
    let to: i64 = v.offset.trim().parse().unwrap_or(v.log_start);
    match removed(v.log_start, v.high_watermark, to) {
        0 => "Delete records".to_owned(),
        1 => "Delete 1 record".to_owned(),
        n => format!("Delete {} records", number(n)),
    }
}

/// The dialog for one partition.
#[must_use]
pub fn delete_records_dialog(v: &DeleteRecordsView, base: &Base, inline: bool) -> Markup {
    modal(
        "delete-records",
        &topic_url(base, &v.topic, "/delete-records"),
        inline,
        html! { "Delete records from " span class="mono" { (format!("{}-{}", v.topic, v.partition)) } },
        body(v),
        html! {
            (cancel(&topic_url(base, &v.topic, "/partitions")))
            (submit("btn danger fill", Some("DeleteRecords"), None, html! { (button_label(v)) }))
        },
    )
}

fn instead(base: &Base, topic: &str) -> Markup {
    card(html! {
        h2 { "What removes records here instead" }
        div class="rules" {
            div class="r ok" { (glyph(Glyph::Check, 16, "")) div { "The cleaner" span class="sub" { "keeps the latest record of every key, drops the rest" } } span class="v" { "compaction" } }
            div class="r ok" { (glyph(Glyph::Check, 16, "")) div { "A tombstone" span class="sub" { "a record with a key and no value; it and the records it deletes go after delete.retention.ms" } } span class="v" { "deletion" } }
            div class="r no" { (glyph(Glyph::X, 16, "")) div { "Retention" span class="sub" { "does not apply: the policy is compact only" } } span class="v" { "not here" } }
        }
        div class="row" style="margin-top:12px" { a class="btn" href=(topic_url(base, topic, "/log")) { "See what compaction did to this log" } }
    })
}

/// The refused state, on a page of its own.
#[must_use]
pub fn delete_records_refused(r: &DeleteRecordsRefused, base: &Base) -> Markup {
    let sentence = if r.refusal.code == 44 {
        format!(
            "Delete records on a topic that only compacts. The partition was not touched: log start is still {}.",
            number(r.log_start)
        )
    } else {
        format!(
            "Delete records was refused. The partition was not touched: log start is still {}.",
            number(r.log_start)
        )
    };
    html! {
        div class="page-h" { div {
            div class="title-row" { h1 class="mono" { (r.topic) } span class="pill mono" { (r.cleanup) } }
            p class="sent" style="margin:10px 0 0;max-width:760px" { (sentence) }
        } }
        div class="split wide" {
            div class="stack-y" {
                (refused(&r.refusal))
                @if r.refusal.code == 44 { (instead(base, &r.topic)) }
            }
            div class="stack-y" { (apicall("The answer", &r.answer, None)) }
        }
    }
}
