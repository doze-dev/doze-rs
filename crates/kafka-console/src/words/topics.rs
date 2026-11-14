//! The words of the topic pages: lists, timeouts, retention and the broker's errors. The sentences that held a live number are
//! gone (CON-805): their numbers are metric slots.

use crate::format::span_ms;

/// `0 and 2`, `0, 1 and 3`, `5`.
#[must_use]
pub fn list_and<T: ToString>(items: &[T]) -> String {
    let all: Vec<String> = items.iter().map(ToString::to_string).collect();
    match all.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// What Kafka said when it refused a read, in its own name and number: `TOPIC_AUTHORIZATION_FAILED (29)`.
#[must_use]
pub fn kafka_error(code: i16) -> String {
    let name = kafka_wire::ResponseError::try_from_code(code)
        .map_or_else(|| "NONE".to_owned(), |e| screaming(&e.to_string()));
    format!("{name} ({code})")
}

/// `TopicAuthorizationFailed` as Kafka writes it: `TOPIC_AUTHORIZATION_FAILED`.
fn screaming(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 6);
    for (i, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// The sentence for a read the broker refused.
#[must_use]
pub fn read_refused(code: i16) -> String {
    let why = match code {
        29 => " The console's principal may describe this topic but not read it.",
        1 => " The offset asked for is outside the log.",
        56 => " The partition is offline after a failed flush.",
        _ => "",
    };
    format!("The broker refused the read: {}.{why}", kafka_error(code))
}

/// `7 d`, or `for ever` for -1.
#[must_use]
pub fn retention(ms: i64) -> String {
    if ms < 0 {
        "for ever".to_owned()
    } else {
        span_ms(ms)
    }
}

#[cfg(test)]
mod tests;
