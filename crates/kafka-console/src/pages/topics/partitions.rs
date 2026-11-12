//! The partitions of a topic: log start, log end, high watermark, last stable offset and size, and the call behind them.

use maud::{Markup, html};

use super::common::{Acts, head, sentence_p, topic_url};
use crate::base::Base;
use crate::format::{bytes, number};
use crate::model::{PartitionRow, TopicHead};
use crate::ui::button::{Size, State, Variant, link, state};
use crate::ui::glyph::Glyph;
use crate::ui::kafka::{apicall, sec};
use crate::ui::notice::{Tone, callout};
use crate::words::partitions_sentence;

fn stable(p: &PartitionRow) -> Markup {
    html! {
        (number(p.last_stable)) " "
        @if p.held() > 0 { (state(State::Warn, &format!("held {}", p.held()))) } @else { (state(State::Ok, "clear")) }
    }
}

fn row(h: &TopicHead, p: &PartitionRow, base: &Base) -> Markup {
    let del = format!(
        "{}/delete-records?partition={}",
        topic_url(base, &h.name),
        p.partition
    );
    html! {
        tr {
            td class="mono" { (h.name) "-" (p.partition) @if p.offline.is_some() { " " (state(State::Bad, "Offline")) } }
            td class="mono r" { (number(p.start)) }
            td class="mono r" { (number(p.end)) }
            td class="mono r" { (number(p.end)) }
            td class="mono r" { (stable(p)) }
            td class="mono r" { (p.bytes.map_or_else(|| "unknown".to_owned(), bytes)) }
            td class="r" { (link(&del, Variant::Plain, Size::Small, None, "Delete records")) }
        }
    }
}

fn call(h: &TopicHead) -> Markup {
    let parts: Vec<String> = h.parts.iter().map(|p| p.partition.to_string()).collect();
    apicall(
        "ListOffsets v10",
        html! {
            span class="k" { "ListOffsets" } " v10  topic " (h.name) ", partitions " (parts.join(" ")) "\n  timestamp: -2  " span class="c" { "earliest: the log start" }
            "\n  timestamp: -1  " span class="c" { "latest: the log end" }
            "\nisolation_level 1  " span class="c" { "read_committed: latest is the last stable offset" }
        },
        None,
    )
}

/// The page.
#[must_use]
pub fn partitions_page(h: &TopicHead, base: &Base) -> Markup {
    html! {
        (head(h, base, Acts::Partitions, sentence_p(&partitions_sentence(h))))
        (callout(Tone::Plain, Glyph::Info, html! {
            b { "One broker." } " This broker leads every partition and holds the only copy, so there are no leaders, replicas or in-sync lists to show. Said once, here."
        }))
        div style="margin-top:24px" {
            table class="t" {
                thead { tr {
                    th scope="col" { "Partition" } th scope="col" class="r" { "Log start" } th scope="col" class="r" { "Log end" }
                    th scope="col" class="r" { "High watermark" } th scope="col" class="r" { "Last stable offset" } th scope="col" class="r" { "Size" }
                    th scope="col" {}
                } }
                tbody { @for p in &h.parts { (row(h, p, base)) } }
            }
        }
        p class="help" style="margin-top:16px" {
            "Log start moves only when records are deleted by offset (DeleteRecords) or by retention. The log end is the next offset to be written; here the high watermark always equals it, because nothing is replicated."
        }
        (sec(32, "Where the numbers come from", None, call(h)))
    }
}
