//! A refused write, in Kafka's own words, and the console's reading of it.
//!
//! The code and its name are Kafka's. The sentence is the broker's own `error_message` for the call; where the answer
//! carries none (DeleteRecords has no message field) it is Kafka's description of the code, from the table below. The
//! "rule" and "what would pass" lines are the console's, written only for refusals this broker really gives and from
//! facts the page has read.

use kafka_wire::ResponseError;

use crate::format::number;
use crate::model::{Refusal, Span};

/// `InvalidTopicException` as Kafka writes it in its documentation: `INVALID_TOPIC_EXCEPTION`.
#[must_use]
pub fn code_name(code: i16) -> String {
    let camel = match ResponseError::try_from_code(code) {
        Some(ResponseError::Unknown(c)) => return format!("UNKNOWN_ERROR_{c}"),
        Some(e) => e.to_string(),
        None => return "NONE".to_owned(),
    };
    let mut out = String::with_capacity(camel.len() + 6);
    let mut prev_lower = false;
    for c in camel.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            out.push('_');
        }
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Kafka's description of the codes the topic writes can answer (its `Errors.java` text), and the generic one.
#[must_use]
pub fn description(code: i16) -> &'static str {
    match code {
        1 => "The requested offset is not within the range of offsets maintained by the server.",
        2 => {
            "This message has failed its CRC checksum, exceeds the valid size, has a null key for a compacted topic, or is otherwise corrupt."
        }
        3 => "This server does not host this topic-partition.",
        7 => "The request timed out.",
        10 => {
            "The request included a message larger than the max message size the server will accept."
        }
        17 => "The request attempted to perform an operation on an invalid topic.",
        21 => "Produce request specified an invalid value for required acks.",
        29 => "Topic authorization failed.",
        31 => "Cluster authorization failed.",
        32 => "The timestamp of the message is out of acceptable range.",
        36 => "Topic with this name already exists.",
        37 => "Number of partitions is below 1.",
        38 => "Replication factor is below 1 or larger than the number of available brokers.",
        39 => "Replica assignment is invalid.",
        40 => "Configuration is invalid.",
        42 => {
            "This most likely occurs because of a request being malformed by the client library or the message was sent to an incompatible broker. See the broker logs for more details."
        }
        44 => "Request parameters do not satisfy the configured policy.",
        56 => "Disk error when trying to access log file on the disk.",
        73 => "Topic deletion is disabled.",
        76 => "The requesting client does not support the compression type of given partition.",
        87 => "This record has failed the validation on broker and hence will be rejected.",
        100 => "This server does not host this topic ID.",
        _ => "The server experienced an unexpected error when processing the request.",
    }
}

/// A refusal with Kafka's code, its name, and the broker's sentence (or the description of the code when there is none).
#[must_use]
pub fn refusal(code: i16, message: Option<&str>) -> Refusal {
    let said = message.map(str::trim).filter(|m| !m.is_empty());
    Refusal {
        code,
        name: code_name(code),
        message: said.map_or_else(|| description(code).to_owned(), str::to_owned),
        rule: None,
        would_pass: None,
    }
}

fn t(s: &str) -> Span {
    Span::text(s)
}

fn m(s: impl Into<String>) -> Span {
    Span::mono(s)
}

fn with(mut r: Refusal, rule: Vec<Span>, would_pass: Vec<Span>) -> Refusal {
    r.rule = (!rule.is_empty()).then_some(rule);
    r.would_pass = (!would_pass.is_empty()).then_some(would_pass);
    r
}

fn not_allowed(r: Refusal, what: &str) -> Refusal {
    with(
        r,
        vec![t(&format!(
            "The console acts as User:__console, and the broker's ACLs do not let it {what}."
        ))],
        vec![t(&format!(
            "an ACL that allows User:__console to {what}, or --acl-super-user User:__console."
        ))],
    )
}

/// The reading of a refused CreateTopics. `most` is the broker's partition limit, if read.
#[must_use]
pub fn create(code: i16, message: Option<&str>, most: Option<i32>) -> Refusal {
    let r = refusal(code, message);
    match code {
        36 => with(
            r,
            vec![t("The name is taken.")],
            vec![t(
                "another name, or open the topic that is there and add partitions to it (partitions can only go up).",
            )],
        ),
        17 => with(
            r,
            vec![
                t(
                    "The name has a character Kafka forbids, or differs from a topic that exists only by ",
                ),
                m("."),
                t(" against "),
                m("_"),
                t(" or by letter case."),
            ],
            vec![
                t("ASCII letters and digits, "),
                m("."),
                t(", "),
                m("_"),
                t(" and "),
                m("-"),
                t(", at most 249 of them, not "),
                m("."),
                t(" or "),
                m(".."),
                t(", and a name that collides with none."),
            ],
        ),
        37 => with(
            r,
            vec![t("The count is outside what this broker allows.")],
            vec![t(&most.map_or_else(
                || "a count of at least 1.".to_owned(),
                |m| format!("a count from 1 to {}.", number(i64::from(m))),
            ))],
        ),
        40 => with(
            r,
            vec![t(
                "A config value Kafka would not accept; the message names the first bad key.",
            )],
            vec![t(
                "a value of the kind Kafka documents for that key, as the Configuration step shows under each field.",
            )],
        ),
        29 => not_allowed(r, "CREATE topics"),
        _ => r,
    }
}

/// The reading of a refused Produce. `compacts` is whether the topic's cleanup.policy compacts, `keyless` whether the
/// record had no key, `limit` the topic's `max.message.bytes` if read.
#[must_use]
pub fn produce(
    code: i16,
    message: Option<&str>,
    compacts: bool,
    keyless: bool,
    limit: Option<i64>,
) -> Refusal {
    let r = refusal(code, message);
    match code {
        87 if compacts && keyless => with(
            r,
            vec![t("The topic's cleanup.policy compacts, and this broker refuses a keyless record instead of storing one the cleaner would drop. The batch was not written; no offset was used.")],
            vec![t("give the record a key. A key with no value is a deletion (a tombstone) and is accepted.")],
        ),
        87 => with(
            r,
            vec![t("The broker's checks on the record failed. The batch was not written; no offset was used.")],
            Vec::new(),
        ),
        10 => with(
            r,
            vec![t("The record is larger than this topic accepts. The batch was not written; no offset was used.")],
            vec![t(&limit.map_or_else(
                || "a smaller record, or a larger max.message.bytes on the topic.".to_owned(),
                |n| format!("a record under {} bytes (max.message.bytes), or a larger limit on the topic.", number(n)),
            ))],
        ),
        3 => with(r, vec![t("The topic or the partition is not here.")], Vec::new()),
        29 => not_allowed(r, "WRITE to the topic"),
        _ => r,
    }
}

/// The reading of a refused CreatePartitions. `have` is the count now, `most` the broker's limit.
#[must_use]
pub fn add_partitions(code: i16, message: Option<&str>, have: i32, most: Option<i32>) -> Refusal {
    let r = refusal(code, message);
    match code {
        37 => with(
            r,
            vec![t(
                "A topic's partition count can only go up. The same count is refused too. Nothing changed.",
            )],
            vec![t(&most.map_or_else(
                || format!("any total above {have}."),
                |m| {
                    format!(
                        "any total from {} to {}.",
                        number(i64::from(have) + 1),
                        number(i64::from(m))
                    )
                },
            ))],
        ),
        29 => not_allowed(r, "ALTER the topic"),
        _ => r,
    }
}

/// The reading of a refused DeleteRecords.
#[must_use]
pub fn delete_records(code: i16, high_watermark: Option<i64>) -> Refusal {
    let r = refusal(code, None);
    match code {
        44 => with(
            r,
            vec![t(
                "On a topic whose cleanup.policy is only compact, records are removed by the cleaner (the latest value of each key stays), not by offset. This broker answers this for every partition of such a topic.",
            )],
            vec![
                t("add "),
                m("delete"),
                t(" to cleanup.policy ("),
                m("delete,compact"),
                t("), then records before an offset can go."),
            ],
        ),
        1 => with(
            r,
            vec![t("The offset is past the high watermark.")],
            vec![t(&high_watermark.map_or_else(
                || "-1, or an offset up to the high watermark.".to_owned(),
                |h| format!("-1, or an offset from 0 to {}.", number(h)),
            ))],
        ),
        29 => not_allowed(r, "DELETE from the topic"),
        _ => r,
    }
}

/// The reading of a refused DeleteTopics.
#[must_use]
pub fn delete_topic(code: i16, message: Option<&str>) -> Refusal {
    let r = refusal(code, message);
    match code {
        3 => with(
            r,
            vec![t("There is no such topic: it may already be gone.")],
            Vec::new(),
        ),
        29 => not_allowed(r, "DELETE the topic"),
        _ => r,
    }
}

#[cfg(test)]
mod tests;
