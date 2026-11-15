//! The words of the Groups pages: the sentence under a group's name, the lag sentences, the list's rebalancing line, Kafka's own
//! descriptions of the codes a group write can answer, and the console's reading of each refusal.
//!
//! The sentences say what the group machine says now and, where the broker's ring of notes has it (slice 9f), why the
//! newest rebalance began; a group the broker has no notes of gives no cause, and nothing is made up.

use crate::format::span_ms;
use crate::model::{GroupAttention, GroupSpan as S};
use crate::words::list_and;

/// `2.1 s`, `45 s`, `3 min`: tenths of a second below a minute.
#[must_use]
pub fn span_precise(ms: u64) -> String {
    if ms < 60_000 {
        if ms % 1000 < 100 {
            return format!("{} s", ms / 1000);
        }
        format!("{}.{} s", ms / 1000, ms % 1000 / 100)
    } else {
        span_ms(i64::try_from(ms).unwrap_or(i64::MAX))
    }
}

mod refusals;
mod story;
pub use refusals::{delete_reading, description, kafka_refusal, reset_reading};
pub use story::attention_sentence;

#[cfg(test)]
mod tests;
