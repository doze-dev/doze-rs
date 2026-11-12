//! The Producers page of a partition (board `topic-producers`) and the page of a transaction (`txn-open`, and `txn-holding` for who
//! is held). The producers a partition remembers, the five batches it keeps for each, the transaction an open one belongs to, what
//! it holds back, and the aborted transactions the partition remembers.
//!
//! What the boards draw that the broker cannot supply, and what the page does instead (debt rows J231 on):
//! - "Seen as", the client name of a producer, comes from the wire recorder (slice 9e): the column says so.
//! - "raised from 1 at 17:14:11": the epoch history is not kept; only the epoch now is shown.
//! - "On the wire", the last requests that touched the topic: the recorder, slice 9e.
//! - "Offsets staged" of a transaction: DescribeTransactions does not list them; the row says so.
//! - How an abort came about (the producer or the timeout): the marker does not say; the row says an abort marker ended it.

mod hold;
mod table;
mod tail;
mod txn;

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::model::ProducersView;
use crate::pages::log::topic_tabs;
use crate::producers_words::producers_sentence;
use crate::ui::button::{State, pill_mono, state};
use crate::ui::client;
use crate::ui::kafka::title_row;
use crate::ui::write::spans;

pub use hold::txn_held_page;
pub use hold::txn_held_url;
pub use txn::{no_txn, txn_page};

/// The fragments of the page that follow the data.
pub const PRODUCERS_SENTENCE: &str = "producers-sentence";
pub const PRODUCERS_TABLE: &str = "producers-table";

/// `/topics/orders/producers?partition=1`.
#[must_use]
pub fn producers_url(base: &Base, topic: &str, partition: i32) -> String {
    let t = base.url(&format!("/topics/{}/producers", segment(topic)));
    if partition == 0 {
        t
    } else {
        format!("{t}?partition={partition}")
    }
}

/// `/topics/orders/transactions/orders-eos`.
#[must_use]
pub fn txn_url(base: &Base, topic: &str, id: &str) -> String {
    base.url(&format!(
        "/topics/{}/transactions/{}",
        segment(topic),
        segment(id)
    ))
}

fn fragment_url(base: &Base, id: &str, v: &ProducersView) -> String {
    base.url(&format!(
        "/fragments/{id}?topic={}&partition={}",
        segment(&v.topic),
        v.partition
    ))
}

fn sentence(v: &ProducersView) -> Markup {
    html! { p class="sub" data-extra="live-sentence" { (spans(&producers_sentence(v))) } }
}

/// The sentence under the name, as a fragment that refreshes while the page is open.
#[must_use]
pub fn producers_sentence_fragment(v: &ProducersView, base: &Base) -> Markup {
    client::fragment(
        PRODUCERS_SENTENCE,
        &fragment_url(base, PRODUCERS_SENTENCE, v),
        sentence(v),
    )
}

/// The table of producers, as a fragment that refreshes while the page is open.
#[must_use]
pub fn producers_table_fragment(v: &ProducersView, base: &Base) -> Markup {
    client::fragment(
        PRODUCERS_TABLE,
        &fragment_url(base, PRODUCERS_TABLE, v),
        table::section(v, base),
    )
}

/// The partition selector: a plain GET form, so it works with no script.
fn partition_seg(v: &ProducersView) -> Markup {
    html! {
        form class="toolbar" method="get" action="" {
            div class="seg" aria-label="Partition" {
                @for p in 0..v.partitions.min(64) {
                    button class={@if p == v.partition { "on" }} type="submit" name="partition" value=(p) data-ui="select" { (v.topic) "-" (p) }
                }
            }
        }
    }
}

/// The page.
#[must_use]
pub fn producers_page(v: &ProducersView, base: &Base) -> Markup {
    html! {
        div class="page-h" {
            div {
                (title_row(&v.topic, html! {
                    @if !v.open.is_empty() { (state(State::Warn, "Transaction open")) " " }
                    (pill_mono(&format!("partition {} of {}", v.partition, v.partitions)))
                }))
                (producers_sentence_fragment(v, base))
            }
            div class="acts" {}
        }
        (topic_tabs(&v.topic, base, "Producers"))
        (partition_seg(v))
        (producers_table_fragment(v, base))
        @if v.held() > 0 { (hold::section(v, base)) }
        (table::aborted(v))
    }
}

#[cfg(test)]
mod tests;
