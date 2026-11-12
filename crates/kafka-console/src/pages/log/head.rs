//! The head of a Log page: the topic's name with its words, the sentence under it and the tabs of a topic with Log showing.

use maud::{Markup, html};

use super::{LOG_SENTENCE, fragment_url};
use crate::base::{Base, segment};
use crate::log_words::{head_sentence, state_word};
use crate::model::{Cleanup, LogView};
use crate::ui::button::{pill_mono, state};
use crate::ui::client;
use crate::ui::kafka::{Tab, tabs, title_row};
use crate::ui::write::spans;

/// The tabs of a topic. Consumers goes to the topic's own page, as the boards draw it.
#[must_use]
pub(crate) fn topic_tabs(topic: &str, base: &Base, on: &str) -> Markup {
    let t = base.url(&format!("/topics/{}", segment(topic)));
    let tab = |label: &'static str, href: String| Tab {
        label,
        href,
        on: label == on,
        count: None,
    };
    tabs(&[
        tab("Messages", t.clone()),
        tab("Log", format!("{t}/log")),
        tab("Config", format!("{t}/config")),
        tab("Consumers", t.clone()),
        tab("Producers", format!("{t}/producers")),
    ])
}

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
    html! {
        (state(tone, word))
        @if v.cleanup == Cleanup::Both { " " (pill_mono("compact,delete")) }
        " " (pill_mono(&format!("partition {} of {}", v.partition, v.partitions)))
    }
}

/// The title row, the sentence and the tabs.
#[must_use]
pub(super) fn log_head(v: &LogView, base: &Base) -> Markup {
    html! {
        div class="page-h" {
            div { (title_row(&v.topic, beside(v))) (log_sentence_fragment(v, base)) }
            div class="acts" {}
        }
        (topic_tabs(&v.topic, base, "Log"))
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
