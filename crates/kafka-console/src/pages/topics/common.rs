//! What the pages of one topic share: its title row, actions, tabs and the open-transaction callout.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::{clock, number};
use crate::model::{Cleanup, Span, TopicHead};
use crate::ui::button::{Size, State, Variant, link, opener, pill_mono, state};
use crate::ui::client;
use crate::ui::glyph::Glyph;
use crate::ui::kafka::{Tab, tabs, title_row};
use crate::ui::layout::page_header;
use crate::ui::notice::{Tone, callout};
use crate::words::{list_and, timeout, topic_sentence};

/// The fragment that holds the sentence under a topic's name.
pub const TOPIC_SENTENCE: &str = "topic-sentence";

/// `/topics/orders`.
#[must_use]
pub fn topic_url(base: &Base, name: &str) -> String {
    base.url(&format!("/topics/{}", segment(name)))
}

pub(super) fn spans(parts: &[Span]) -> Markup {
    html! {
        @for p in parts {
            @match p {
                Span::Text(t) => (t),
                Span::Mono(t) => span class="mono" { (t) },
                Span::Strong(t) => b { (t) },
            }
        }
    }
}

/// The policy as the pill says it.
#[must_use]
pub(super) fn policy_word(c: Cleanup) -> &'static str {
    match c {
        Cleanup::Delete => "delete",
        Cleanup::Compact => "compact",
        Cleanup::Both => "compact,delete",
    }
}

/// Which buttons the head carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Acts {
    /// Partitions, Produce, Delete topic.
    Full,
    /// Produce and Delete topic: a topic with nothing in it has no partitions page worth opening.
    Empty,
    /// Partitions and Produce: a topic with an offline partition (the board does not draw Delete there).
    Offline,
    /// Add partitions.
    Partitions,
}

fn words(h: &TopicHead) -> Markup {
    let down: Vec<i32> = h.offline().iter().map(|p| p.partition).collect();
    html! {
        (pill_mono(policy_word(h.cleanup)))
        @if !down.is_empty() {
            " " (state(State::Bad, &format!("Partition{} {} offline", if down.len() == 1 { "" } else { "s" }, list_and(&down))))
        } @else if !h.txns.is_empty() {
            " " (state(State::Warn, "Transaction open"))
        }
    }
}

fn buttons(h: &TopicHead, base: &Base, acts: Acts) -> Markup {
    let t = topic_url(base, &h.name);
    let partitions = link(
        &format!("{t}/partitions"),
        Variant::Plain,
        Size::Normal,
        Some(Glyph::List),
        "Partitions",
    );
    let produce = opener(
        &format!("{t}/produce"),
        Variant::Primary,
        Size::Normal,
        Some(Glyph::Send),
        "Produce",
    );
    let delete = opener(
        &format!("{t}/delete"),
        Variant::Danger,
        Size::Normal,
        Some(Glyph::Trash),
        "Delete topic",
    );
    html! {
        @match acts {
            Acts::Full => { (partitions) (produce) (delete) },
            Acts::Empty => { (produce) (delete) },
            Acts::Offline => { (partitions) (produce) },
            Acts::Partitions => { (opener(&format!("{t}/partitions/add"), Variant::Primary, Size::Normal, Some(Glyph::Plus), "Add partitions")) },
        }
    }
}

/// The sentence under the name.
#[must_use]
pub(super) fn sentence_p(sentence: &[Span]) -> Markup {
    html! { p class="sent" data-extra="live-sentence" style="margin:10px 0 0;max-width:760px" { (spans(sentence)) } }
}

/// The sentence under the name, as a fragment that refreshes while the page is open.
#[must_use]
pub fn sentence_fragment(h: &TopicHead, base: &Base) -> Markup {
    client::fragment(
        TOPIC_SENTENCE,
        &base.url(&format!(
            "/fragments/{TOPIC_SENTENCE}?topic={}",
            segment(&h.name)
        )),
        sentence_p(&topic_sentence(h)),
    )
}

/// The head of a topic page: the name with its words, the sentence that follows the topic, and the buttons.
#[must_use]
pub(super) fn head(h: &TopicHead, base: &Base, acts: Acts, sentence: Markup) -> Markup {
    let title = html! {
        div {
            (title_row(&h.name, if acts == Acts::Partitions { html! {} } else { words(h) }))
            (sentence)
        }
    };
    html! {
        div class="page-h" { (title) div class="acts" { (buttons(h, base, acts)) } }
    }
}

/// The head with the topic's own sentence.
#[must_use]
pub(super) fn topic_head(h: &TopicHead, base: &Base, acts: Acts) -> Markup {
    head(h, base, acts, sentence_fragment(h, base))
}

/// The tabs of a topic: Messages is the one showing.
#[must_use]
pub(super) fn topic_tabs(h: &TopicHead, base: &Base) -> Markup {
    let t = topic_url(base, &h.name);
    tabs(&[
        Tab {
            label: "Messages",
            href: t.clone(),
            on: true,
            count: None,
        },
        Tab {
            label: "Log",
            href: format!("{t}/log"),
            on: false,
            count: None,
        },
        Tab {
            label: "Config",
            href: format!("{t}/config"),
            on: false,
            count: None,
        },
        Tab {
            label: "Consumers",
            href: base.url("/groups"),
            on: false,
            count: Some(h.tabs.consumers),
        },
        Tab {
            label: "Producers",
            href: format!("{t}/producers"),
            on: false,
            count: Some(h.tabs.producers),
        },
    ])
}

/// The callout of an open transaction (the topic page and no other).
#[must_use]
pub(super) fn txn_callout(h: &TopicHead, base: &Base) -> Markup {
    let Some(t) = h.txns.first() else {
        return html! {};
    };
    let on: Vec<String> = t
        .partitions
        .iter()
        .map(|(p, _)| format!("{}-{p}", h.name))
        .collect();
    let held = h
        .parts
        .iter()
        .filter(|p| p.held() > 0)
        .max_by_key(|p| p.held());
    // The transaction's own page (slice 9g).
    let txn_url = format!(
        "{}/transactions/{}",
        topic_url(base, &h.name),
        crate::base::segment(&t.id)
    );
    html! {
        div style="margin-bottom:24px" {
            (callout(Tone::Warn, Glyph::Warn, html! {
                b { "Transaction " (t.id) " is open." }
                " Producer " (t.producer_id) " (epoch " (t.producer_epoch) ") began it at " (clock(t.since_ms)) " UTC on "
                (list_and(&on)) "; the timeout is " (timeout(t.timeout_ms)) ". "
                @if let Some(p) = held {
                    "The last stable offset of " (h.name) "-" (p.partition) " is " (number(p.last_stable))
                    " while its high watermark is " (number(p.end)) ", so a read_committed consumer is held "
                    (p.held()) " records behind. "
                }
                a class="lnk" href=(txn_url) { "Open the transaction" }
            }))
        }
    }
}

/// The page for a name that is no topic.
#[must_use]
pub fn unknown(name: &str, base: &Base) -> Markup {
    html! {
        (page_header("No such topic", html! { p class="sub" { "There is no topic called " span class="mono" { (name) } ", or the console may not describe it." } }, html! {}))
        div class="row" {
            (link(&base.url("/topics"), Variant::Primary, Size::Normal, Some(Glyph::ArrowR), "Back to Topics"))
            (link(&base.url("/jump"), Variant::Plain, Size::Normal, Some(Glyph::Search), "Find a topic"))
        }
    }
}
