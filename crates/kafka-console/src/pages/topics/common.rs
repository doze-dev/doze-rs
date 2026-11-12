//! What the pages of one topic share: its title row, actions, tabs and the open-transaction callout.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::bytes;
use crate::format::{clock, number};
use crate::model::{Cleanup, Span, TopicHead};
use crate::ui::button::{Size, State, Variant, link, opener, pill_mono, state};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::kafka::{Tab, tabs, title_row};
use crate::ui::layout::page_header;
use crate::ui::metrics::Stat;
use crate::ui::metrics::stat_strip;
use crate::ui::notice::{Tone, callout};
use crate::words::{list_and, timeout};

/// The region that holds a topic's facts and its transaction line.
pub const TOPIC_STATS: &str = "topic-stats";

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
pub enum Acts {
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
        @if h.cleanup == Cleanup::Compact { (state(State::Ok, "Compacted")) } @else { (pill_mono(policy_word(h.cleanup))) }
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

/// The facts of a topic as a strip of metric slots, and the transaction region under it, as a fragment that a Refresh fetches
/// again. No sentence holds a number (CON-805) and the region under the strip has the same height whether a transaction is open or
/// not (CON-807).
#[must_use]
pub fn stats_fragment(h: &TopicHead, base: &Base) -> Markup {
    client::fragment(
        TOPIC_STATS,
        &base.url(&format!(
            "/fragments/{TOPIC_STATS}?topic={}",
            segment(&h.name)
        )),
        html! { (stats_strip(h)) (txn_region(h, base)) },
    )
}

fn stats_strip(h: &TopicHead) -> Markup {
    let n = |v: usize| number(i64::try_from(v).unwrap_or(0));
    let retention = h.retention_ms.map_or_else(
        || policy_word(h.cleanup).to_owned(),
        |ms| {
            format!(
                "{} · {}",
                policy_word(h.cleanup),
                crate::words::retention(ms)
            )
        },
    );
    let last = match h.last_write_ms() {
        Some(ms) if !h.is_empty() => Stat::at("Last write", ms, h.now_ms),
        _ => Stat::new("Last write", "never").mono().ch(11),
    };
    stat_strip(&[
        Stat::new("Partitions", n(h.parts.len())).ch(4),
        Stat::new("Offline", n(h.offline().len())).ch(4),
        Stat::new("Retention", retention).mono().ch(14),
        Stat::new("Records", number(h.records())).ch(9),
        Stat::new("Size on disk", bytes(h.bytes())).ch(9),
        last,
    ])
}

/// The open-transaction region: the callout, or a calm line, in a block of the same height.
fn txn_region(h: &TopicHead, base: &Base) -> Markup {
    html! { div class="txn-region" { (txn_callout(h, base)) } }
}

/// The head of a topic page: the name with its words, the sentence that follows the topic, and the buttons.
#[must_use]
pub(super) fn head(h: &TopicHead, base: &Base, acts: Acts, stats: Markup) -> Markup {
    let title = html! {
        div { (title_row(&h.name, if acts == Acts::Partitions { html! {} } else { words(h) })) }
    };
    html! {
        div class="page-h" { (title) div class="acts" { (buttons(h, base, acts)) } }
        (stats)
    }
}

/// The head with the topic's own sentence.
#[must_use]
pub(super) fn topic_head(h: &TopicHead, base: &Base, acts: Acts) -> Markup {
    head(h, base, acts, stats_fragment(h, base))
}

/// The five tabs of a topic: the one showing is `on`. The same five links, in the same order, with the same counts, on every
/// page of the topic, so that a tab switch changes only what is under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicTab {
    Messages,
    Log,
    Config,
    Consumers,
    Producers,
}

/// The tabs of a topic with `on` showing.
#[must_use]
pub fn topic_tabs_on(h: &TopicHead, base: &Base, on: TopicTab) -> Markup {
    let t = topic_url(base, &h.name);
    let tab = |tab: TopicTab, label: &'static str, href: String, count: Option<usize>| Tab {
        label,
        href,
        on: tab == on,
        count,
    };
    tabs(&[
        tab(TopicTab::Messages, "Messages", t.clone(), None),
        tab(TopicTab::Log, "Log", format!("{t}/log"), None),
        tab(TopicTab::Config, "Config", format!("{t}/config"), None),
        tab(
            TopicTab::Consumers,
            "Consumers",
            format!("{t}/consumers"),
            Some(h.tabs.consumers),
        ),
        tab(
            TopicTab::Producers,
            "Producers",
            format!("{t}/producers"),
            Some(h.tabs.producers),
        ),
    ])
}

/// The tabs of a topic: Messages is the one showing.
#[must_use]
pub(super) fn topic_tabs(h: &TopicHead, base: &Base) -> Markup {
    topic_tabs_on(h, base, TopicTab::Messages)
}

/// Which buttons the head carries: a function of the topic and nothing else, so that it is the same on every tab.
#[must_use]
pub fn acts_of(h: &TopicHead) -> Acts {
    if !h.offline().is_empty() {
        Acts::Offline
    } else if h.is_empty() {
        Acts::Empty
    } else {
        Acts::Full
    }
}

/// The head and the tabs of a topic, as one piece: the name with its words, the sentence that follows the topic, the
/// buttons, and the five tabs with `on` showing. Every tab page starts with exactly this, so switching tabs changes nothing
/// above the tab body.
#[must_use]
pub fn topic_frame(h: &TopicHead, base: &Base, on: TopicTab) -> Markup {
    html! {
        (topic_head(h, base, acts_of(h)))
        (topic_tabs_on(h, base, on))
    }
}

/// The callout of an open transaction (the topic page and no other).
#[must_use]
pub(super) fn txn_callout(h: &TopicHead, base: &Base) -> Markup {
    let Some(t) = h.txns.first() else {
        return html! { div class="calm" { (glyph(Glyph::Check, 16, "")) "No open transactions." } };
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
        div {
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
