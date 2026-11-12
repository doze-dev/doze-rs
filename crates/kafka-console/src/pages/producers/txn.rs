//! The page of one transaction (`txn-open`): what it is, how long it has left, what it holds back, what happens if it is not
//! ended, and every transaction the broker knows.

use maud::{Markup, html};

use super::hold::txn_held_url;
use super::tail::ruler;
use super::{producers_url, txn_url};
use crate::base::Base;
use crate::format::{clock, number, span_ms};
use crate::model::{TopicHead, TxnRow, TxnView};
use crate::pages::log::lead;
use crate::pages::topics::{TopicTab, topic_frame};
use crate::producers_words::{
    holds_sentence, left_ms, running_ms, timeout_percent, timeout_text, txn_sentence, unended_parts,
};
use crate::ui::button::{Size, State, Variant, link, pill_mono, state};
use crate::ui::glyph::Glyph;
use crate::ui::layout::{kv, value_with_sub};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::spans;

const STATES: [&str; 6] = [
    "Ongoing",
    "PrepareCommit",
    "PrepareAbort",
    "CompleteCommit",
    "CompleteAbort",
    "Empty",
];

fn tone(s: &str) -> State {
    match s {
        "Ongoing" => State::Warn,
        "CompleteCommit" => State::Ok,
        "PrepareAbort" | "CompleteAbort" => State::Idle,
        _ => State::Info,
    }
}

/// The topic's head and tabs with Producers on, then what is about this transaction: its state, the sentence, and the actions.
pub(super) fn txn_head(
    v: &TxnView,
    head: &TopicHead,
    base: &Base,
    sentence: Markup,
    acts: Markup,
) -> Markup {
    html! {
        (topic_frame(head, base, TopicTab::Producers))
        (lead(
            html! { @if v.txn.ongoing() { (state(State::Warn, "Transaction open")) } @else { (state(tone(&v.txn.state), &v.txn.state)) } },
            html! { div class="row sb" { p class="sub" data-extra="live-sentence" { (sentence) } div class="acts" { (acts) } } },
        ))
    }
}

fn partitions_cell(v: &TxnView) -> Markup {
    html! {
        @for (i, p) in v.parts.iter().enumerate() {
            @if i == 0 { span class="mono" { (v.topic) "-" (p.partition) @if let Some(f) = p.first { " from " (number(f)) } } }
            @else { span class="sub mono" { (v.topic) "-" (p.partition) @if let Some(f) = p.first { " from " (number(f)) } } }
        }
        @for (t, p) in &v.elsewhere { span class="sub mono" { (t) "-" (p) } }
        @if v.parts.is_empty() && v.elsewhere.is_empty() { span class="dim" { "none yet" } }
    }
}

fn the_transaction(v: &TxnView) -> Markup {
    let x = &v.txn;
    let running = running_ms(x, v.now_ms);
    let left = left_ms(x, v.now_ms);
    html! {
        div class="card" {
            h2 { "The transaction" }
            (kv(vec![
                ("Transactional id", html! { span class="mono" { (x.id) } }),
                ("State", html! { (state(tone(&x.state), &x.state)) }),
                ("Producer", html! { span class="mono" { (x.producer_id) ", epoch " (x.producer_epoch) } }),
                ("Began", value_with_sub(html! { span class="mono" { (clock(x.since_ms)) } }, &format!("{} ago; adding partitions does not start the clock again", span_ms(running)))),
                ("Timeout", html! {
                    div class="lagbar" style="max-width:360px" { div class="tr" { i style={"--p:" (timeout_percent(x, v.now_ms)) "%"} {} } span class="n" { (span_ms(running)) " of " (timeout_text(x.timeout_ms)) } }
                    span class="sub" { (timeout_text(x.timeout_ms)) ": " @if left > 0 { (span_ms(left)) " left, then the broker aborts it and fences the producer" } @else { "passed: the broker aborts it at its next look and fences the producer" } }
                }),
                ("Partitions", partitions_cell(v)),
                ("Offsets staged", value_with_sub(html! { "not listed" }, "DescribeTransactions does not say whether a group was added; on commit staged offsets are applied to the group, on abort forgotten")),
            ]))
        }
    }
}

fn counts(all: &[TxnRow]) -> Vec<(&'static str, usize)> {
    STATES
        .iter()
        .map(|s| (*s, all.iter().filter(|t| t.state == *s).count()))
        .collect()
}

fn all_table(v: &TxnView, base: &Base) -> Markup {
    html! {
        div class="sec" style="margin-top:36px" {
            h2 { "Transactions on this broker" }
            p class="lede" { "The broker knows " b { (v.all.len() + v.more) } @if v.all.len() + v.more == 1 { " transactional id" } @else { " transactional ids" } ", by state. ListTransactions answers by state, producer id and how long a transaction has run." }
            div class="toolbar" {
                div class="seg" {
                    button class="on" type="button" data-ui="filter" { "All " (v.all.len() + v.more) }
                    @for (s, n) in counts(&v.all) { button type="button" data-ui="filter" { (s) " " (n) } }
                }
            }
            table class="t" {
                thead { tr { th scope="col" { "Transactional id" } th scope="col" { "State" } th scope="col" { "Producer" } th scope="col" { "Running" } th scope="col" { "Timeout" } th scope="col" { "Partitions" } } }
                tbody {
                    @for t in &v.all {
                        tr {
                            td class="name mono" { a class="lnk" href=(txn_url(base, &v.topic, &t.id)) { (t.id) } }
                            td { (state(tone(&t.state), &t.state)) }
                            td class="mono" { (t.producer_id) ", epoch " (t.producer_epoch) }
                            td class="mono" data-vary="" { (span_ms(running_ms(t, v.now_ms))) }
                            td class="mono" { (timeout_text(t.timeout_ms)) }
                            td class="mono" { (partitions_of(t)) }
                        }
                    }
                }
            }
            p class="help" style="margin-top:12px" {
                "A transactional id is forgotten after 7 days idle (" span class="mono" { "transactional.id.expiration.ms" } "), once its transaction is over; one with a transaction open never is, because the timeout deals with it first. If it is forgotten, the next InitProducerId makes a new producer id."
                @if v.more > 0 { " " (v.more) " more are not listed." }
            }
        }
    }
}

fn partitions_of(t: &TxnRow) -> String {
    t.partitions
        .iter()
        .map(|(n, p)| format!("{n}-{p}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What happens if it is not ended: the refusal a fenced producer meets is drawn as every refusal is.
fn unended(v: &TxnView) -> Markup {
    let (before, after) = unended_parts(v);
    html! {
        p class="sent" {
            (spans(&before))
            @if !after.is_empty() { (state(State::Refused, "Refused")) " " span class="mono" { "INVALID_PRODUCER_EPOCH (47)" } (spans(&after)) }
        }
    }
}

/// The page of the transaction.
#[must_use]
pub fn txn_page(v: &TxnView, head: &TopicHead, base: &Base) -> Markup {
    let held = v.held();
    html! {
        (txn_head(v, head, base, spans(&txn_sentence(v)), html! {}))
        div class="split wide" {
            (the_transaction(v))
            div class="stack-y" {
                div class="card" {
                    h2 { "What it holds back" }
                    p class="sent" { (spans(&holds_sentence(v))) }
                    div class="row" style="margin-top:14px" { a class="btn sm" href=(txn_held_url(base, &v.topic, &v.txn.id)) { "Who is held" } }
                }
                div class="card" { h2 { "If it is not ended" } (unended(v)) }
            }
        }
        @if held > 0 || !v.tail.cells.is_empty() {
            div class="sec" style="margin-top:36px" {
                h2 { (v.topic) "-" (v.tail_partition) ", the tail" }
                p class="lede" { "Offsets " (number(v.tail.from)) " to " (number(v.tail.from + i64::try_from(v.tail.cells.len()).unwrap_or(0) - 1)) "." }
                @if let Some(p) = v.parts.iter().find(|p| p.partition == v.tail_partition) { (ruler(&v.tail, p.last_stable, p.end)) }
            }
        }
        (callout(Tone::Plain, Glyph::Info, html! { "Only the producer, by ending the transaction, or the timeout can end it. The console only reads it: ending it is the producer's job, or the timeout's." }))
        (all_table(v, base))
    }
}

/// The page for a transaction the broker does not know.
#[must_use]
pub fn no_txn(topic: &str, id: &str, base: &Base) -> Markup {
    html! {
        (crate::ui::layout::page_header("No such transaction", html! { p class="sub" { "The broker knows no transaction called " span class="mono" { (id) } ", or the console may not describe it. A transactional id is forgotten after 7 days idle." } }, html! {}))
        div class="row" { (link(&producers_url(base, topic, 0), Variant::Primary, Size::Normal, Some(Glyph::ArrowR), "Back to the producers")) (pill_mono(topic)) }
    }
}
