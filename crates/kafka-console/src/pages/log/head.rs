//! The head of a Log page: the topic's name with its words, the sentence under it and the tabs of a topic with Log showing.

use maud::{Markup, html};

use super::{LOG_SENTENCE, fragment_url};
use crate::base::Base;
use crate::log_words::{head_sentence, state_word};
use crate::model::{Cleanup, LogState, LogView, TopicHead};
use crate::pages::topics::{TopicTab, topic_frame};
use crate::ui::button::{pill_mono, state};
use crate::ui::client;
use crate::ui::write::spans;

/// The sentence under the name, as a fragment that refreshes while the page is open.
#[must_use]
pub fn log_sentence_fragment(v: &LogView, base: &Base) -> Markup {
    client::fragment(
        LOG_SENTENCE,
        &fragment_url(base, LOG_SENTENCE, v),
        sentence(v),
    )
}

fn sentence(v: &LogView) -> Markup {
    html! { p class="sub" data-extra="live-sentence" { (spans(&head_sentence(v))) } }
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

/// What this tab says first, under the topic's own head and tabs (the same on every tab of the topic): the words of the log (due,
/// compacted, which partition) and the sentence that follows the data.
#[must_use]
pub(crate) fn lead(words: Markup, sentence: Markup) -> Markup {
    html! {
        div class="tab-lead" {
            div class="row" style="gap:8px;align-items:center;margin-bottom:8px" { (words) }
            (sentence)
        }
    }
}

/// The topic's head and tabs with Log on, the lead, and the partition chips.
#[must_use]
pub(super) fn log_head(v: &LogView, head: &TopicHead, base: &Base) -> Markup {
    html! {
        (topic_frame(head, base, TopicTab::Log))
        (lead(beside(v), log_sentence_fragment(v, base)))
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
