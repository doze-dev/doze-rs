//! The Log page of a partition (boards `topic-log`, `log-planner`, `log-retention`, `log-empty`): the data chooses which. The
//! ruler of its offsets with the gaps compaction left, the rules of the cleaner's planner or of retention with this
//! partition's numbers, the last round or pass, the deletions a round has kept, how the log is kept on disk, and its segments.
//!
//! What the boards draw that the broker cannot supply, and what the page does instead (debt rows J231 on):
//! - "Compact now" is Lab (a fault rule in the request path): not built, and no button stands where it stood.
//! - "On the wire", the last requests that touched the topic, needs the request recorder (slice 9e).
//! - "Flushed through offset N": the log does not keep how far its flush got, so the durability card says how the log is
//!   flushed and what the last stop left, not an offset.
//! - The first entries of a segment's index: no accessor shows them; the row says how many there should be, about.

mod disk;
mod empty;
mod head;
mod others;
mod planner;
mod retention;
mod ruler;

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::model::{LogState, LogView, TopicHead};
use crate::ui::client;

pub(crate) use head::lead;
use head::log_head;
pub use head::log_sentence_fragment;

/// The fragments of the page that follow the data.
pub const LOG_SENTENCE: &str = "log-sentence";
pub const LOG_RULER: &str = "log-ruler";
pub const LOG_CARDS: &str = "log-cards";

/// `/topics/orders/log?partition=1`.
#[must_use]
pub fn log_url(base: &Base, topic: &str, partition: i32) -> String {
    let t = base.url(&format!("/topics/{}/log", segment(topic)));
    if partition == 0 {
        t
    } else {
        format!("{t}?partition={partition}")
    }
}

/// Where a fragment of the page is fetched.
pub(super) fn fragment_url(base: &Base, id: &str, v: &LogView) -> String {
    base.url(&format!(
        "/fragments/{id}?topic={}&partition={}",
        segment(&v.topic),
        v.partition
    ))
}

/// The ruler, as a fragment that refreshes while the page is open.
#[must_use]
pub fn log_ruler_fragment(v: &LogView, base: &Base) -> Markup {
    client::fragment(
        LOG_RULER,
        &fragment_url(base, LOG_RULER, v),
        ruler::section(v, base),
    )
}

/// What the cleaner or retention will do and did: the cards beside each other, as a fragment.
#[must_use]
pub fn log_cards_fragment(v: &LogView, base: &Base) -> Markup {
    client::fragment(LOG_CARDS, &fragment_url(base, LOG_CARDS, v), cards(v, base))
}

fn cards(v: &LogView, base: &Base) -> Markup {
    match v.state() {
        LogState::Empty => html! {},
        LogState::Due => planner::due_cards(v, base),
        LogState::NotDue => planner::not_due_cards(v, base),
        LogState::Retention => retention::cards(v),
    }
}

/// The page.
#[must_use]
pub fn log_page(v: &LogView, head: &TopicHead, base: &Base, listen: &str) -> Markup {
    let state = v.state();
    html! {
        (log_head(v, head, base))
        @if state == LogState::NotDue { (planner::not_due_callout(v)) }
        (log_ruler_fragment(v, base))
        @if state == LogState::Empty {
            (empty::empty(v, base, listen))
        } @else {
            (log_cards_fragment(v, base))
            (disk::durability_and_segments(v, base))
        }
        @if state != LogState::Empty { (others::others(v, base)) }
    }
}

#[cfg(test)]
mod tests;
