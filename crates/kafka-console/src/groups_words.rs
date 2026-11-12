//! The words of the Groups pages: the sentence under a group's name, the lag sentences, the list's rebalancing line, Kafka's own
//! descriptions of the codes a group write can answer, and the console's reading of each refusal.
//!
//! The sentences say what the group machine says now and, where the broker's ring of notes has it (slice 9f), why the
//! newest rebalance began; a group the broker has no notes of gives no cause, and nothing is made up.

use crate::format::{number, span_ms};
use crate::model::{GroupAttention, GroupSpan as S};
use crate::words::list_and;

/// `one`, `two`, ... `ten`, else the digits.
#[must_use]
pub fn number_word(n: usize) -> String {
    const WORDS: [&str; 11] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    WORDS.get(n).map_or_else(
        || number(i64::try_from(n).unwrap_or(i64::MAX)),
        |w| (*w).to_owned(),
    )
}

/// `1 member`, `3 members`.
#[must_use]
pub fn members_word(n: usize) -> String {
    if n == 1 {
        "1 member".to_owned()
    } else {
        format!("{n} members")
    }
}

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

/// A timeout the way a person says it: `45 s`, `60 s`, `3 min`.
#[must_use]
pub fn seconds(ms: u64) -> String {
    if ms < 120_000 && ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else if ms < 60_000 {
        span_precise(ms)
    } else {
        span_ms(i64::try_from(ms).unwrap_or(i64::MAX))
    }
}
