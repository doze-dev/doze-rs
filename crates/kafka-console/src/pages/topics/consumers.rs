//! The Consumers tab of a topic: the consumer groups that have committed offsets on it, with where each stands on every
//! partition of this topic and how far behind that is. The same words and arithmetic as the Groups pages (`crate::lag`).

use maud::{Markup, html};

use super::common::{TopicTab, topic_frame, topic_url};
use crate::base::{Base, segment};
use crate::format::number;
use crate::model::{ConsumersView, GroupProtocol, TopicReader};
use crate::pages::groups::{cell_lag, tone};
use crate::ui::button::state;
use crate::ui::glyph::Glyph;
use crate::ui::kafka::empty;

/// The id of the region the tab's data is in.
pub const CONSUMERS_REGION: &str = "topic-consumers";

fn bits(r: &TopicReader, topic: &str) -> Markup {
    html! {
        (state(tone(r.phase), r.phase.word()))
        " " span class="pill" { @if r.protocol == GroupProtocol::Consumer { "consumer protocol" } @else { "classic" } }
        span class="dim" { "\u{a0}"
            (crate::format::count(r.members, "member", "members"))
            @if r.members > 0 { ", " (r.holding) " hold" @if r.holding == 1 { "s" } " " (topic) }
        }
    }
}

fn group_head(r: &TopicReader, v: &ConsumersView, base: &Base) -> Markup {
    let behind = r.block.behind;
    html! {
        tr id=(format!("cg-{}", r.id)) { td colspan="4" class="grp" {
            a class="lnk mono" href=(base.url(&format!("/groups/{}", segment(&r.id)))) { (r.id) }
            span class="grp-bits" { (bits(r, &v.head.name)) }
            span class="mono bright grp-n" { @if behind > 0 { (number(behind)) " behind" } @else { "0 behind" } }
        } }
    }
}

fn partition_row(group: &str, p: &crate::model::LagRow) -> Markup {
    html! {
        tr id=(format!("cp-{group}-{}-{}", p.topic, p.partition)) {
            td class="name mono" { (p.topic) "-" (p.partition) }
            td class="r mono" { @match p.end { Some(e) => (number(e)), None => span class="dim" { "—" } } }
            @match p.committed {
                Some(c) => td class="r mono" { (number(c)) },
                None => td class="r dim" { "none" },
            }
            td { (cell_lag(p)) }
        }
    }
}

fn table(v: &ConsumersView, base: &Base) -> Markup {
    html! {
        table class="t consumers" data-extra="lag-line" {
            thead { tr {
                th scope="col" { "Partition" } th scope="col" class="r" { "Log end" } th scope="col" class="r" { "Committed" } th scope="col" { "Lag" }
            } }
            tbody {
                @for r in &v.readers {
                    (group_head(r, v, base))
                    @for p in &r.block.rows { (partition_row(&r.id, p)) }
                }
            }
        }
    }
}

fn sentence(v: &ConsumersView) -> Markup {
    let n = v.total;
    let last = v.readers.iter().filter_map(|r| r.last_commit_ms).max();
    html! {
        p class="sent" style="margin:0 0 16px;max-width:900px" {
            b { (n) } " consumer " (if n == 1 { "group has" } else { "groups have" }) " committed offsets on " span class="mono" { (v.head.name) } "."
            @if let Some(ms) = last { " The latest commit was at " (crate::format::stamp(ms, v.head.now_ms)) "." }
            @if v.readers.len() < n { " The " (v.readers.len()) " furthest behind are listed." }
        }
    }
}

fn nobody(v: &ConsumersView, base: &Base) -> Markup {
    empty(
        Glyph::Queue,
        "d-msg",
        None,
        html! { "No consumer group has committed offsets for " span class="mono" { (v.head.name) } },
        html! { "A group shows up here the first time it commits an offset on this topic. A consumer that reads without committing is not listed." },
        html! { a class="btn" href=(format!("{}/messages", topic_url(base, &v.head.name))) { "Read the messages" } },
    )
}

/// The page.
#[must_use]
pub fn consumers_page(v: &ConsumersView, base: &Base) -> Markup {
    html! {
        (topic_frame(&v.head, base, TopicTab::Consumers))
        div id=(CONSUMERS_REGION) {
            @if v.readers.is_empty() { (nobody(v, base)) } @else { (sentence(v)) (table(v, base)) }
        }
    }
}
