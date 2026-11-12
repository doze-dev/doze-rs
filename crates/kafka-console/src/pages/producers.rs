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
use crate::format::number;
use crate::model::{ProducersView, TopicHead};
use crate::pages::log::lead;
use crate::pages::topics::{TopicTab, topic_frame};
use crate::ui::button::pill_mono;
use crate::ui::client;
use crate::ui::metrics::{Stat, stat_strip};

pub use hold::txn_held_page;
pub use hold::txn_held_url;
pub use txn::{no_txn, txn_page};

/// The fragments of the page that follow the data.
pub const PRODUCERS_STATS: &str = "producers-stats";
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

/// What follows the partition's producers, as a strip of metric slots in a region a Refresh fetches again (CON-805).
#[must_use]
pub fn producers_stats_fragment(v: &ProducersView, base: &Base) -> Markup {
    let n = |x: i64| number(x);
    client::fragment(
        PRODUCERS_STATS,
        &fragment_url(base, PRODUCERS_STATS, v),
        stat_strip(&[
            Stat::new(
                "Producers",
                n(i64::try_from(v.producers.len()).unwrap_or(0)),
            )
            .ch(5),
            Stat::new(
                "Open transactions",
                n(i64::try_from(v.open.len()).unwrap_or(0)),
            )
            .ch(4),
            Stat::new("Last stable", n(v.last_stable)).ch(9),
            Stat::new("High watermark", n(v.end)).ch(9),
            Stat::new("Held back", n(v.held())).unit("records").ch(7),
        ]),
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
pub fn producers_page(v: &ProducersView, head: &TopicHead, base: &Base) -> Markup {
    html! {
        (topic_frame(head, base, TopicTab::Producers))
        (lead(html! {
            (pill_mono(&format!("partition {} of {}", v.partition, v.partitions)))
        }, producers_stats_fragment(v, base)))
        (partition_seg(v))
        (producers_table_fragment(v, base))
        @if v.held() > 0 { (hold::section(v, base)) }
        (table::aborted(v))
    }
}

#[cfg(test)]
mod tests;
