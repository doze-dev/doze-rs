//! The producers table with the batches behind each producer, and the aborted transactions the partition remembers.

use maud::{Markup, html};

use super::{producers_url, txn_url};
use crate::base::Base;
use crate::format::{clock, date_time, number, span_ms};
use crate::model::{ProdBatch, ProdRow, ProducersView};
use crate::ui::button::{State, state};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_log::{LogGlyph, log_glyph};

fn is_open(v: &ProducersView, p: &ProdRow) -> bool {
    v.opened.contains(&p.id) || (v.opened.is_empty() && p.txn.is_some())
}

/// The link that shows or hides a producer's batches: a plain GET, so it works with no script.
fn toggle(v: &ProducersView, p: &ProdRow, base: &Base, open: bool) -> Markup {
    let mut ids: Vec<i64> = if v.opened.is_empty() {
        v.producers
            .iter()
            .filter(|x| x.txn.is_some())
            .map(|x| x.id)
            .collect()
    } else {
        v.opened.clone()
    };
    if open {
        ids.retain(|i| *i != p.id);
    } else {
        ids.push(p.id);
    }
    let mut href = producers_url(base, &v.topic, v.partition);
    let q: Vec<String> = ids.iter().map(ToString::to_string).collect();
    href.push(if href.contains('?') { '&' } else { '?' });
    href.push_str("open=");
    href.push_str(&q.join(","));
    html! {
        a class="btn sm ghost" href=(href) aria-expanded=(open) {
            @if open { (log_glyph(LogGlyph::ChevD, 14, "")) "Hide batches" } @else { (glyph(Glyph::ChevR, 14, "")) "Batches" }
        }
    }
}

fn batch_row(b: &ProdBatch) -> Markup {
    html! {
        tr {
            td class="mono" { (number(i64::from(b.first_sequence))) " to " (number(i64::from(b.last_sequence)))
                span class="sub" { (b.records()) @if b.records() == 1 { " record" } @else { " records" } @if b.open { ", open" } } }
            td class="mono" { (number(b.first_offset)) " to " (number(b.last_offset)) }
            td class="mono" { (clock(b.at_ms)) }
        }
    }
}

fn batches(v: &ProducersView, p: &ProdRow) -> Markup {
    let last = p.batches.first();
    html! {
        tr {
            td colspan="7" style="background:var(--well)" {
                div class="stack-y" style="gap:14px;padding:8px 4px" {
                    div class="row sb" {
                        b { "Producer " (p.id) ", the last " (p.batches.len()) @if p.batches.len() == 1 { " batch " } @else { " batches " } (v.topic) "-" (v.partition) " remembers" }
                        span class="dim" {
                            @if let Some(t) = &p.txn { "open from " (t.from.map_or_else(String::new, number)) "; " }
                            @if let Some(b) = last { "last write " (clock(b.at_ms)) }
                        }
                    }
                    table class="t" {
                        thead { tr { th scope="col" { "Sequences" } th scope="col" { "Offsets" } th scope="col" { "At" } } }
                        tbody { @for b in &p.batches { (batch_row(b)) } }
                    }
                    p class="help" style="max-width:none" {
                        "A batch sent again with one of these sequences is answered with where it went, not written twice. A gap or an overlap in the sequences, or a repeat older than the "
                        (p.batches.len().max(5)) " remembered, is " (state(State::Refused, "Refused")) " " span class="mono" { "OUT_OF_ORDER_SEQUENCE_NUMBER (45)" }
                        "; a batch at an epoch below the producer's is " (state(State::Refused, "Refused")) " " span class="mono" { "INVALID_PRODUCER_EPOCH (47)" } "."
                    }
                }
            }
        }
    }
}

fn row(v: &ProducersView, p: &ProdRow, base: &Base) -> Markup {
    let open = is_open(v, p);
    html! {
        tr {
            td class="name mono" { (p.id) }
            td class="mono" { (p.epoch) }
            td class="mono" { (p.last_sequence.map_or_else(|| "unknown".to_owned(), |s| number(i64::from(s)))) }
            td class="mono" data-vary="" { (span_ms((v.now_ms - p.last_used_ms).max(0))) }
            td {
                @if let Some(t) = &p.txn {
                    a class="lnk" href=(txn_url(base, &v.topic, &t.id)) { (t.id) }
                    span class="sub" { (t.state) @if let Some(f) = t.from { ", from offset " (number(f)) } }
                } @else { span class="dim" { "none" } span class="sub" { "idempotent only" } }
            }
            td class="dim" { "not known" span class="sub" { "a name appears when the wire recorder has seen the producer write" } }
            td class="r" { (toggle(v, p, base, open)) }
        }
        @if open { (batches(v, p)) }
    }
}

/// `Producers on orders-0` with its table.
#[must_use]
pub(super) fn section(v: &ProducersView, base: &Base) -> Markup {
    html! {
        div class="sec" {
            h2 { "Producers on " (v.topic) "-" (v.partition) }
            p class="lede" { "What DescribeProducers says: the id, its epoch, the last sequence the partition accepted, and when it last wrote. Open a row to see the last batches the partition remembers for that producer." }
            table class="t" {
                thead { tr {
                    th scope="col" { "Producer id" } th scope="col" { "Epoch" } th scope="col" { "Last sequence" } th scope="col" { "Idle" }
                    th scope="col" { "Transaction" } th scope="col" { "Seen as" } th scope="col" {}
                } }
                tbody { @for p in &v.producers { (row(v, p, base)) } }
            }
            @if v.producers.is_empty() { p class="help" style="margin-top:12px" { "No producer has written to this partition lately." } }
            p class="help" style="margin-top:12px" { "The broker keeps producer ids, not client ids. A producer id silent for 1 day is forgotten on the partition." }
        }
    }
}

/// `Aborted transactions remembered: N`.
#[must_use]
pub(super) fn aborted(v: &ProducersView) -> Markup {
    let last = v.aborted.iter().filter_map(|a| a.at_ms).max();
    html! {
        div class="sec" {
            h2 { "Aborted transactions remembered: " (v.aborted.len()) }
            p class="lede" {
                @if let Some(t) = last { "The last was aborted " b { (span_ms((v.now_ms - t).max(0))) } " ago. " }
                "An abort is remembered until the log start passes it; a commit is not. read_committed consumers are told these ranges and skip them; read_uncommitted consumers still see the records."
            }
            table class="t" {
                thead { tr { th scope="col" { "Aborted" } th scope="col" { "Producer" } th scope="col" { "Partition" } th scope="col" { "Records" } th scope="col" { "How" } } }
                tbody {
                    @for a in &v.aborted {
                        tr {
                            td class="mono" { @if let Some(t) = a.at_ms { (date_time(t)) span class="sub" data-vary="" { (span_ms((v.now_ms - t).max(0))) " ago" } } @else { "not known" } }
                            td class="mono" { (a.producer) }
                            td class="mono" { (v.topic) "-" (a.partition) }
                            td class="mono" { (number(a.first)) " to " (number(a.marker - 1)) span class="sub" { "marker at " (number(a.marker)) } }
                            td { (state(State::Idle, "Abort")) span class="sub" { "an abort marker ended it; the log does not say whether the producer or the timeout wrote it" } }
                        }
                    }
                }
            }
            @if v.aborted.is_empty() { p class="help" style="margin-top:12px" { "No aborted transaction is remembered on this partition." } }
        }
    }
}
