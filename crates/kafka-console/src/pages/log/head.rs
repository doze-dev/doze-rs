//! The head of a Log page: the topic's name with its words, the sentence under it and the tabs of a topic with Log showing.

use maud::{Markup, html};

use super::{LOG_STATS, fragment_url};
use crate::base::Base;
use crate::format::{bytes, number};
use crate::log_words::state_word;
use crate::model::{Cleanup, LogState, LogView, TopicHead};
use crate::pages::topics::{TopicTab, topic_frame};
use crate::ui::button::{pill_mono, state};
use crate::ui::client;
use crate::ui::metrics::{Stat, stat_strip};

/// What follows the log, as a strip of metric slots in a region a Refresh fetches again (CON-805).
#[must_use]
pub fn log_stats_fragment(v: &LogView, base: &Base) -> Markup {
    let n = |x: i64| number(x);
    let state = match v.state() {
        LogState::Empty => "Empty",
        LogState::Due => "Due",
        LogState::NotDue => "Not due",
        LogState::Retention => "Retention",
    };
    client::fragment(
        LOG_STATS,
        &fragment_url(base, LOG_STATS, v),
        stat_strip(&[
            Stat::new("Log", state).ch(10),
            Stat::new("Start offset", n(v.start)).ch(9),
            Stat::new("End offset", n(v.end)).ch(9),
            Stat::new("Last stable", n(v.last_stable)).ch(9),
            Stat::new("Records", n(v.records())).ch(9),
            Stat::new("Segments", n(i64::try_from(v.segments.len()).unwrap_or(0))).ch(4),
            Stat::new("Size", bytes(v.bytes())).ch(9),
        ]),
    )
}

fn beside(v: &LogView) -> Markup {
    let (tone, word) = state_word(v);
    // The topic's own head already says Compacted, Delete and an open transaction; what only this page knows is that the log is
    // empty or that the cleaner will not take it yet.
    html! {
        @if matches!(v.state(), LogState::Empty | LogState::NotDue) { (state(tone, word)) " " }
        @if v.cleanup == Cleanup::Both { (pill_mono("compact,delete")) " " }
        (pill_mono(&format!("partition {} of {}", v.partition, v.partitions)))
    }
}

/// What this tab says first, under the topic's own head and tabs: the words of the log (due, compacted, which partition), then the
/// strip of metrics that follows the data.
#[must_use]
pub(crate) fn lead(words: Markup, stats: Markup) -> Markup {
    html! {
        div class="tab-lead" {
            div class="row" style="gap:8px;align-items:center;margin-bottom:12px" { (words) }
            (stats)
        }
    }
}

/// The topic's head and tabs with Log on, the lead, and the partition chips.
#[must_use]
pub(super) fn log_head(v: &LogView, head: &TopicHead, base: &Base) -> Markup {
    html! {
        (topic_frame(head, base, TopicTab::Log))
        (lead(beside(v), log_stats_fragment(v, base)))
        (partitions(v, base))
    }
}

/// A topic with more than one partition says which it is showing and links to the others.
fn partitions(v: &LogView, base: &Base) -> Markup {
    if v.partitions <= 1 {
        return html! {};
    }
    html! {
        nav class="toolbar" aria-label="Partition" {
            span class="dim" { "Partition" }
            @for p in 0..v.partitions.min(64) {
                a class={"fchip" @if p == v.partition { " on" }} href=(super::log_url(base, &v.topic, p))
                    aria-current=[(p == v.partition).then_some("page")] { (p) }
            }
        }
    }
}
