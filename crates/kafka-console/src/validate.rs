//! The checks a form makes before it sends anything, and the sentences it says when one fails. Pure functions.
//!
//! These exist so that a typo is caught where it was typed. They never stand in for the broker: what passes here is
//! still put to Kafka, whose answer (or refusal, in its own words) is what the page shows. The topic-name rule is the
//! broker's own (`TopicName::parse`), not a copy of it.

use kafka_wire::ids::{MAX_TOPIC_NAME_LEN, TopicName, TopicNameError};

use crate::format::{number, span_ms};

/// A character as a sentence names it: printable ones in quotes, the rest by code point.
fn shown(c: char) -> String {
    if c.is_control() || c == '\u{fffd}' || c.is_whitespace() {
        format!("U+{:04X}", u32::from(c))
    } else {
        format!("'{c}'")
    }
}

/// Why `raw` cannot be a topic's name, in a sentence; `Ok` if Kafka would allow it.
///
/// # Errors
/// The sentence, naming the first character that is not allowed.
pub fn topic_name(raw: &str) -> Result<(), String> {
    match TopicName::parse(raw) {
        Ok(_) => Ok(()),
        Err(TopicNameError::Empty) => Err("A topic needs a name.".to_owned()),
        Err(TopicNameError::Reserved) => Err("'.' and '..' cannot be topic names.".to_owned()),
        Err(TopicNameError::TooLong) => Err(format!(
            "{} characters is over the limit of {MAX_TOPIC_NAME_LEN}.",
            number(i64::try_from(raw.chars().count()).unwrap_or(i64::MAX))
        )),
        Err(_) => {
            let bad = raw
                .chars()
                .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')));
            Err(match bad {
                Some(c) => format!(
                    "{} is not allowed: ASCII letters and digits, '.', '_' and '-' only.",
                    shown(c)
                ),
                None => "Only ASCII letters and digits, '.', '_' and '-' are allowed.".to_owned(),
            })
        }
    }
}

/// How a name meets the topics that are there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// Nothing in the way.
    Free,
    /// A topic has exactly this name.
    Exists,
    /// A topic differs only by `.` against `_` (they share a metric name) or by letter case (they share a directory).
    Collides(String),
}

fn fold(name: &str) -> String {
    name.to_ascii_lowercase().replace('.', "_")
}

/// Whether `name` is free among `existing`. Advisory: the broker decides, on the same two rules.
#[must_use]
pub fn taken<'a>(name: &str, existing: impl IntoIterator<Item = &'a str>) -> Taken {
    let folded = fold(name);
    let mut collides = None;
    for other in existing {
        if other == name {
            return Taken::Exists;
        }
        if collides.is_none() && fold(other) == folded {
            collides = Some(other.to_owned());
        }
    }
    collides.map_or(Taken::Free, Taken::Collides)
}

/// A whole number the person typed, or the sentence that asks for one.
fn whole(raw: &str) -> Result<i64, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("Type a whole number.".to_owned());
    }
    t.parse::<i64>()
        .map_err(|_| "A whole number is needed, such as 3.".to_owned())
}

/// A partition count for a new topic: at least 1, and at most the broker's limit if it is known.
///
/// # Errors
/// The sentence.
pub fn partitions(raw: &str, most: Option<i32>) -> Result<i32, String> {
    let n = whole(raw)?;
    if n < 1 {
        return Err("A topic has at least 1 partition.".to_owned());
    }
    match (i32::try_from(n), most) {
        (Ok(n), Some(m)) if n > m => Err(format!(
            "{} is above this broker's limit of {} (--max-partitions).",
            number(i64::from(n)),
            number(i64::from(m))
        )),
        (Ok(n), _) => Ok(n),
        (Err(_), _) => {
            Err("That is more partitions than a number of this size can hold.".to_owned())
        }
    }
}

/// The line under the count when it is not above the partitions there are (the broker refuses both): the board's words.
#[must_use]
pub fn not_above(wanted: i32, have: i32) -> Option<String> {
    match wanted.cmp(&have) {
        std::cmp::Ordering::Less => Some(format!(
            "{} is below the {} partitions there are.",
            number(i64::from(wanted)),
            number(i64::from(have))
        )),
        std::cmp::Ordering::Equal => Some(format!(
            "{} is the number of partitions there are already.",
            number(i64::from(wanted))
        )),
        std::cmp::Ordering::Greater => None,
    }
}

/// A partition count to put to the broker as typed: only that it is a whole number that fits, so that what the broker
/// refuses (0, a count over its limit) is refused in the broker's words and not in the console's.
///
/// # Errors
/// The sentence, when it is not a number at all.
pub fn count_to_send(raw: &str) -> Result<i32, String> {
    let n = whole(raw)?;
    i32::try_from(n)
        .map_err(|_| "That is more partitions than a number of this size can hold.".to_owned())
}

/// `retention.ms`: -1 (no limit), or 0 and up.
///
/// # Errors
/// The sentence.
pub fn retention_ms(raw: &str) -> Result<i64, String> {
    let n = whole(raw)?;
    if n < -1 {
        return Err("-1 (no limit), or 0 and up.".to_owned());
    }
    Ok(n)
}

/// The words beside a `retention.ms` figure: `= 7 d`, `= no limit`. Empty if it is not a figure.
#[must_use]
pub fn retention_words(raw: &str) -> String {
    match whole(raw) {
        Ok(-1) => "= no limit".to_owned(),
        Ok(n) if n >= 0 => format!("= {}", span_ms(n)),
        _ => String::new(),
    }
}

/// The offset to delete before: -1 (the high watermark: everything) or 0 and up.
///
/// # Errors
/// The sentence.
pub fn delete_before(raw: &str) -> Result<i64, String> {
    let n = whole(raw)?;
    if n < -1 {
        return Err("An offset from 0 up, or -1 for the high watermark.".to_owned());
    }
    Ok(n)
}

/// `acks`: -1 (all), 1 or 0.
///
/// # Errors
/// The sentence.
pub fn acks(raw: &str) -> Result<i16, String> {
    match raw.trim() {
        "-1" => Ok(-1),
        "1" => Ok(1),
        "0" => Ok(0),
        _ => Err("acks is -1 (all), 1 or 0.".to_owned()),
    }
}

/// The partition to write to: empty is "let the key choose", else an index below the topic's count.
///
/// # Errors
/// The sentence.
pub fn partition_choice(raw: &str, count: i32) -> Result<Option<i32>, String> {
    if raw.trim().is_empty() || raw.trim() == "auto" {
        return Ok(None);
    }
    let n = whole(raw)?;
    match i32::try_from(n) {
        Ok(p) if (0..count).contains(&p) => Ok(Some(p)),
        _ => Err(format!(
            "This topic has partitions 0 to {}.",
            count.saturating_sub(1)
        )),
    }
}

#[cfg(test)]
mod tests;
