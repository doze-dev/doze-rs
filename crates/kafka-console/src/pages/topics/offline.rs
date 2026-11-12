//! A topic with a partition that a failed flush took offline (`topic-offline`).
//!
//! The broker keeps the cause of the failure and not the time of it, so the page says what failed and not when. The wire
//! recorder's section ("what the wire recorded") is slice 9e.

use maud::{Markup, html};

use super::common::{Acts, topic_head, topic_tabs, topic_url};
use crate::base::Base;
use crate::model::TopicHead;
use crate::ui::button::{State, state};
use crate::ui::glyph::{Glyph, glyph};

fn works(h: &TopicHead) -> Markup {
    html! {
        div class="card" {
            h2 { "What works and what does not" }
            table class="t" style="margin-top:12px" {
                thead { tr { th scope="col" { "Partition" } th scope="col" { "State" } th scope="col" { "Writes" } th scope="col" { "Reads" } } }
                tbody {
                    @for p in &h.parts { tr {
                        td class="mono" { (p.partition) }
                        @if p.offline.is_some() {
                            td { (state(State::Bad, "Offline")) }
                            td { (state(State::Refused, "Refused")) " " span class="mono" { "KAFKA_STORAGE_ERROR (56)" } }
                        } @else {
                            td { (state(State::Ok, "Online")) }
                            td { "accepted" }
                        }
                        td { "served" }
                    } }
                }
            }
        }
    }
}

fn restart() -> Markup {
    html! {
        div class="card" {
            h2 { "What restarting does" }
            p class="lede" { "An offline partition stays offline until restart. Restart the machine, not only doze." }
            p style="margin:0" { "After a failed flush the kernel may mark the lost pages clean. A new doze process on the same machine can find them in the page cache, and its flush then succeeds without writing anything. Only starting the machine again clears that." }
            p style="margin:12px 0 0" class="dim" { "Records acknowledged with acks -1 were flushed before the failure. With acks 1 the last 500 ms before it may not have been." }
        }
    }
}

/// The page: the cause in a banner, what still works, and what restarting does.
#[must_use]
pub(super) fn offline_page(h: &TopicHead, base: &Base) -> Markup {
    let down = h.offline();
    html! {
        (topic_head(h, base, Acts::Offline))
        (topic_tabs(h, base))
        @for p in &down { div class="banner bad" {
            (glyph(Glyph::Warn, 18, ""))
            span { b { (h.name) "-" (p.partition) " is offline." } " A flush of its active segment failed: " span class="mono" { (p.offline.as_deref().unwrap_or("")) } ". The broker does not retry it." }
        } }
        div class="g2" { (works(h)) (restart()) }
        p class="help" style="margin-top:24px" {
            "Its records can still be read: "
            a class="lnk" href=(format!("{}/messages?partition={}", topic_url(base, &h.name), down.first().map_or(0, |p| p.partition))) { "open partition " (down.first().map_or(0, |p| p.partition)) }
            "."
        }
    }
}
