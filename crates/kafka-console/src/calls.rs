//! The calls the forms send and the answers they get, written as the boards draw them (`apicall`): an API name and
//! version, then its fields, indented. Pure functions from the values of a form to lines of tokens; the HTML is the
//! page's.
//!
//! What a person typed reaches these lines, so every value goes through [`visible`] first: a newline or a control
//! character cannot make a second line, and nothing is left to look like markup (the page escapes it again).

use crate::model::{Lines, Refusal, Tok};

type Line = Vec<(Tok, String)>;

/// `s` on one line, with control characters written out (`\n`, `\u{0}`), cut after `most` characters.
#[must_use]
pub fn visible(s: &str, most: usize) -> String {
    let mut out = String::new();
    for (n, c) in s.chars().enumerate() {
        if n == most {
            out.push_str(" ...");
            return out;
        }
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

fn pad(depth: usize) -> String {
    "  ".repeat(depth)
}

fn head(api: &str, version: i16, suffix: &str) -> Line {
    vec![
        (Tok::Keyword, api.to_owned()),
        (Tok::Plain, format!(" v{version}{suffix}")),
    ]
}

fn plain(depth: usize, text: &str) -> Line {
    vec![(Tok::Plain, format!("{}{text}", pad(depth)))]
}

fn kv(depth: usize, key: &str, tok: Tok, value: &str) -> Line {
    vec![
        (Tok::Plain, format!("{}{key}: ", pad(depth))),
        (tok, value.to_owned()),
    ]
}

fn num(depth: usize, key: &str, value: impl std::fmt::Display) -> Line {
    kv(depth, key, Tok::Plain, &value.to_string())
}

/// What a `CreateTopics` call sends. `configs` are `(key, value)` in order; none draws the board's "all defaults".
#[must_use]
pub fn create_topics(
    version: i16,
    name: &str,
    partitions: &str,
    configs: &[(&str, &str)],
    validate_only: bool,
) -> Lines {
    let mut out = vec![
        head("CreateTopics", version, ""),
        plain(1, "topics[0]"),
        kv(2, "name", Tok::Str, &visible(name, 80)),
        kv(2, "num_partitions", Tok::Plain, &visible(partitions, 12)),
        num(2, "replication_factor", 1),
    ];
    if configs.is_empty() {
        out.push(kv(2, "configs", Tok::Comment, "(none yet: all defaults)"));
    } else {
        out.push(plain(2, "configs:"));
        for (key, value) in configs {
            out.push(vec![
                (Tok::Plain, pad(3)),
                (Tok::Str, visible(key, 60)),
                (Tok::Plain, format!(" = {}", visible(value, 40))),
            ]);
        }
    }
    out.push(num(1, "validate_only", validate_only));
    out
}

/// The answer to one topic of a CreateTopics.
#[must_use]
pub fn create_topics_answer(version: i16, name: &str, r: &Refusal) -> Lines {
    vec![
        head("CreateTopics", version, " response"),
        plain(1, "topics[0]"),
        kv(2, "name", Tok::Str, &visible(name, 80)),
        kv(2, "error_code", Tok::Str, &format!("{} {}", r.code, r.name)),
        kv(2, "error_message", Tok::Plain, &visible(&r.message, 160)),
        num(2, "num_partitions", -1),
        num(2, "replication_factor", -1),
    ]
}

/// What a `Produce` call sends: one batch of one record.
#[must_use]
pub fn produce(version: i16, topic: &str, partition: i32, acks: i16, r: &RecordText<'_>) -> Lines {
    let mut out = vec![
        head("Produce", version, ""),
        num(1, "acks", acks),
        num(1, "timeout_ms", 30_000),
        plain(1, "topic_data[0]"),
        kv(2, "name", Tok::Str, &visible(topic, 80)),
        plain(2, "partition_data[0]"),
        num(3, "index", partition),
        plain(3, "records: 1 batch, 1 record"),
    ];
    out.push(match r.key {
        Some(k) => kv(4, "key", Tok::Str, &visible(k, 40)),
        None => kv(4, "key", Tok::Comment, "(none)"),
    });
    out.push(match r.value {
        Some(v) => kv(4, "value", Tok::Plain, &visible(v, 32)),
        None => kv(4, "value", Tok::Comment, "(none: a deletion)"),
    });
    if !r.headers.is_empty() {
        let shown: Vec<String> = r
            .headers
            .iter()
            .map(|(k, v)| format!("{} = {}", visible(k, 40), visible(v, 40)))
            .collect();
        out.push(kv(4, "headers", Tok::Plain, &shown.join(", ")));
    }
    out
}

/// The parts of the record a Produce call draws, as text.
#[derive(Debug, Clone, Copy)]
pub struct RecordText<'a> {
    pub key: Option<&'a str>,
    pub value: Option<&'a str>,
    pub headers: &'a [(String, String)],
}

/// What a Produce answered, for one partition.
#[must_use]
pub fn produce_answer(
    version: i16,
    topic: &str,
    partition: i32,
    code: i16,
    name: &str,
    base_offset: i64,
    more: &[(&str, String)],
) -> Lines {
    let ok = code == 0;
    let mut out = vec![
        head("Produce", version, " response"),
        kv(1, "topic", Tok::Str, &visible(topic, 80)),
        num(1, "partition", partition),
        if ok {
            num(1, "error_code", 0)
        } else {
            kv(1, "error_code", Tok::Str, &format!("{code} {name}"))
        },
        if ok {
            kv(1, "base_offset", Tok::Str, &base_offset.to_string())
        } else {
            num(1, "base_offset", base_offset)
        },
    ];
    out.extend(more.iter().map(|(k, v)| kv(1, k, Tok::Plain, v)));
    out
}

/// What a `CreatePartitions` call sends.
#[must_use]
pub fn create_partitions(version: i16, topic: &str, count: &str, validate_only: bool) -> Lines {
    vec![
        head("CreatePartitions", version, ""),
        plain(1, "topics[0]"),
        kv(2, "name", Tok::Str, &visible(topic, 80)),
        kv(2, "count", Tok::Plain, &visible(count, 12)),
        kv(2, "assignments", Tok::Plain, "null"),
        num(1, "validate_only", validate_only),
    ]
}

/// What a CreatePartitions answered.
#[must_use]
pub fn create_partitions_answer(version: i16, topic: &str, r: &Refusal) -> Lines {
    vec![
        head("CreatePartitions", version, " response"),
        kv(1, "name", Tok::Str, &visible(topic, 80)),
        kv(1, "error_code", Tok::Str, &format!("{} {}", r.code, r.name)),
        kv(1, "error_message", Tok::Plain, &visible(&r.message, 160)),
    ]
}

/// What a `DeleteRecords` call sends.
#[must_use]
pub fn delete_records(version: i16, topic: &str, partition: i32, offset: &str) -> Lines {
    vec![
        head("DeleteRecords", version, ""),
        plain(1, "topics[0]"),
        kv(2, "name", Tok::Str, &visible(topic, 80)),
        plain(2, "partitions[0]"),
        num(3, "partition_index", partition),
        kv(3, "offset", Tok::Plain, &visible(offset, 24)),
        num(1, "timeout_ms", 30_000),
    ]
}

/// What a DeleteRecords answered.
#[must_use]
pub fn delete_records_answer(
    version: i16,
    topic: &str,
    partition: i32,
    low: i64,
    r: &Refusal,
) -> Lines {
    vec![
        head("DeleteRecords", version, " response"),
        kv(1, "topic", Tok::Str, &visible(topic, 80)),
        num(1, "partition_index", partition),
        num(1, "low_watermark", low),
        kv(1, "error_code", Tok::Str, &format!("{} {}", r.code, r.name)),
    ]
}

/// What a `DeleteTopics` call sends.
#[must_use]
pub fn delete_topics(version: i16, topic: &str) -> Lines {
    vec![
        head("DeleteTopics", version, ""),
        plain(1, "topics[0]"),
        kv(2, "name", Tok::Str, &visible(topic, 80)),
    ]
}

/// A word for a shell: bare if it is safe, else in single quotes with each quote written `'\''`.
#[must_use]
pub fn shell_word(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'=' | b'/' | b':' | b',')
        });
    if safe {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''").replace('\n', " "))
    }
}

/// `kafka-topics.sh --create`, the same call from a terminal.
#[must_use]
pub fn create_command(
    address: &str,
    name: &str,
    partitions: &str,
    configs: &[(&str, &str)],
) -> Vec<String> {
    let mut lines = vec![
        "kafka-topics.sh --bootstrap-server ".to_owned() + &shell_word(address) + " \\",
        format!(
            "  --create --topic {} --partitions {}{}",
            shell_word(name),
            shell_word(partitions),
            if configs.is_empty() { "" } else { " \\" }
        ),
    ];
    for (i, (k, v)) in configs.iter().enumerate() {
        let tail = if i + 1 == configs.len() { "" } else { " \\" };
        lines.push(format!(
            "  --config {}{tail}",
            shell_word(&format!("{k}={v}"))
        ));
    }
    lines
}

/// The `kcat` lines of the created page: produce one record, and read it in a group.
#[must_use]
pub fn kcat_produce(address: &str, topic: &str) -> Vec<String> {
    let (a, t) = (shell_word(address), shell_word(topic));
    vec![
        "echo '{\"order_id\":\"ord-88420\"}' \\".to_owned(),
        format!("  | kcat -P -b {a} \\"),
        format!("    -t {t} -k ord-88420"),
    ]
}

/// `kcat -C -G`, a consumer in a group.
#[must_use]
pub fn kcat_consume(address: &str, topic: &str) -> Vec<String> {
    let (a, t) = (shell_word(address), shell_word(topic));
    vec![
        format!("kcat -C -b {a} \\"),
        format!(
            "  -G {} {t}",
            shell_word(&format!("{}-workers", topic.replace(['.', '_'], "-")))
        ),
    ]
}

#[cfg(test)]
mod tests;
