//! The Live tail page's stream (`GET /api/v1/tail/topics`, CON-1580 to CON-1585): which topics it follows, where each of their
//! partitions stands, and what each event says. The reading is `gather::tail`; the filters are [`crate::tail_filter`].
//!
//! Bounds, per connection: at most [`MOST_TOPICS`] topics and [`MOST_PARTITIONS`] partitions are followed; one look reads at
//! most [`LOOK_BYTES`] of records in at most [`LOOK_FETCHES`] Fetch requests, round robin over the partitions that have
//! something new, so a busy one never starves the rest; a partition more than [`SKIP_PAST`] records behind is moved to its
//! newest [`SKIP_TO`] and the records passed over are counted, never read. An event carries at most [`EVENT_ROWS`] records.

use kafka_wire::Uuid;

use crate::form::get as field;
use crate::model::{HeaderRow, Reading, Shown, Standing};
use crate::tail_filter::{self, Filter, MOST_TEXT};

pub const MOST_TOPICS: usize = 32;
pub const MOST_PARTITIONS: usize = 512;
/// The most names and patterns a tail is asked for.
pub const MOST_ASKED: usize = 32;
/// The most records "last N per partition" starts with, per partition.
pub const MOST_LAST: i64 = 200;
pub const LOOK_BYTES: usize = 4 << 20;
pub const LOOK_FETCHES: usize = 64;
pub const SKIP_PAST: i64 = 20_000;
pub const SKIP_TO: i64 = 1_000;
pub const EVENT_ROWS: usize = 200;
/// The most headers of a record an event carries (the record Sheet has them all).
pub const EVENT_HEADERS: usize = 16;

/// Where a tail starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// The end of every partition: only what is written from now.
    Now,
    /// The last `n` records of every partition, then what is written.
    Last(i64),
}

/// What a person asked to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub names: Vec<String>,
    pub patterns: Vec<String>,
    pub start: Start,
    /// Only these partitions (of every topic); empty: all.
    pub partitions: Vec<i32>,
    pub filter: Filter,
    pub reading: Reading,
}

fn text_of(f: &[(String, String)], name: &str) -> Option<String> {
    field(f, name)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.chars().take(MOST_TEXT).collect())
}

fn all_of(f: &[(String, String)], name: &str) -> impl Iterator<Item = String> {
    f.iter()
        .filter(move |(k, _)| k == name)
        .map(|(_, v)| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The filters of `?key=&value=&header=&header_value=&field=&equals=`.
fn filter_of(f: &[(String, String)]) -> Result<Filter, String> {
    let field_text = text_of(f, "field");
    let path = match &field_text {
        Some(t) => Some(
            tail_filter::path(t)
                .ok_or("A JSON field is a path like customer.id or items[0].sku.")?,
        ),
        None => None,
    };
    let header = text_of(f, "header");
    let filter = Filter {
        key: text_of(f, "key"),
        value: text_of(f, "value"),
        header_value: header.as_ref().and(text_of(f, "header_value")),
        header,
        equals: path.as_ref().and(text_of(f, "equals")),
        field: path,
    };
    Ok(filter)
}

impl Ask {
    /// The ask in `?topic=a&topic=b&pattern=orders.*&from=now|last&last=10&partition=0&key=&value=&header=&header_value=
    /// &field=&equals=&isolation=read_committed`; `Err` with the words of what is wrong.
    ///
    /// # Errors
    /// No topic and no pattern; a name or a pattern that cannot be one; a path that is not one.
    pub fn parse(f: &[(String, String)], valid: impl Fn(&str) -> bool) -> Result<Self, String> {
        let mut names: Vec<String> = Vec::new();
        for n in all_of(f, "topic") {
            if !valid(&n) {
                return Err(format!(
                    "{} cannot be a topic name.",
                    n.chars().take(80).collect::<String>()
                ));
            }
            if !names.contains(&n) {
                names.push(n);
            }
        }
        let mut patterns: Vec<String> = Vec::new();
        for p in all_of(f, "pattern") {
            if !tail_filter::valid_pattern(&p) {
                return Err("A pattern is the characters of a topic name, with * for any run of them and ? for one.".into());
            }
            if !patterns.contains(&p) {
                patterns.push(p);
            }
        }
        if names.is_empty() && patterns.is_empty() {
            return Err("Name a topic to follow, or a pattern of names.".into());
        }
        if names.len() + patterns.len() > MOST_ASKED {
            return Err(format!(
                "A tail follows at most {MOST_ASKED} names and patterns."
            ));
        }
        let start = if field(f, "from") == Some("last") {
            let n = field(f, "last")
                .and_then(|n| n.trim().parse::<i64>().ok())
                .unwrap_or(10);
            Start::Last(n.clamp(1, MOST_LAST))
        } else {
            Start::Now
        };
        let mut partitions: Vec<i32> = all_of(f, "partition")
            .filter_map(|p| p.parse().ok())
            .filter(|p| *p >= 0)
            .collect();
        partitions.sort_unstable();
        partitions.dedup();
        let filter = filter_of(f)?;
        let reading = if field(f, "isolation") == Some("read_committed") {
            Reading::Committed
        } else {
            Reading::Uncommitted
        };
        Ok(Self {
            names,
            patterns,
            start,
            partitions,
            filter,
            reading,
        })
    }

    /// Whether the topic `name` is one this tail follows.
    #[must_use]
    pub fn wants(&self, name: &str, internal: bool) -> bool {
        self.names.iter().any(|n| n == name)
            || (!internal && self.patterns.iter().any(|p| tail_filter::glob(p, name)))
    }
}

/// A followed partition and the next offset to read on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub topic: String,
    pub id: Uuid,
    pub partition: i32,
    pub next: i64,
    /// Where the partition ended at the last look (its last stable offset for `read_committed`).
    pub end: i64,
}

/// One tail's state between looks.
#[derive(Debug, Clone)]
pub struct Follow {
    pub ask: Ask,
    pub places: Vec<Place>,
    /// The topics followed, by name, with their partition counts.
    pub topics: Vec<TailTopic>,
    /// Where the next look's round robin starts.
    pub turn: usize,
    /// The cursor a reconnect brought (`Last-Event-ID`), used by the first look only.
    pub resume: Option<Vec<(String, i32, i64)>>,
    /// The first look is done: a topic found after it joins at its start (all of it is new).
    pub opened: bool,
    /// Topics that matched but were not followed (past [`MOST_TOPICS`] or [`MOST_PARTITIONS`]).
    pub left_out: usize,
}

impl Follow {
    #[must_use]
    pub fn new(ask: Ask, resume: Option<&str>) -> Self {
        Self {
            ask,
            places: Vec::new(),
            topics: Vec::new(),
            turn: 0,
            resume: resume.and_then(decode),
            opened: false,
            left_out: 0,
        }
    }

    /// The cursor of every partition followed: `orders:0=12,1=40;payments:0=3` (the event's `id`).
    #[must_use]
    pub fn cursor(&self) -> String {
        let mut out = String::new();
        let mut last: Option<&str> = None;
        for p in &self.places {
            if last == Some(p.topic.as_str()) {
                out.push(',');
            } else {
                if last.is_some() {
                    out.push(';');
                }
                out.push_str(&p.topic);
                out.push(':');
                last = Some(&p.topic);
            }
            out.push_str(&format!("{}={}", p.partition, p.next));
        }
        out
    }
}

/// The places of a cursor written by [`Follow::cursor`]; `None` if it is not one.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<(String, i32, i64)>> {
    let mut out = Vec::new();
    for topic in text.trim().split(';').filter(|t| !t.is_empty()) {
        let (name, parts) = topic.split_once(':')?;
        for pair in parts.split(',') {
            let (p, o) = pair.split_once('=')?;
            out.push((name.to_owned(), p.parse().ok()?, o.parse().ok()?));
        }
        if out.len() > MOST_PARTITIONS {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A record one look read, as far as it was kept (its key to 1 KiB, its value to 8 KiB, each header value to 256 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub timestamp_ms: i64,
    pub key: Option<Vec<u8>>,
    pub key_len: u64,
    pub value: Option<Vec<u8>>,
    pub value_len: u64,
    pub headers: Vec<(Vec<u8>, Option<Vec<u8>>, u64)>,
    pub standing: Standing,
}

/// What one look did: the records it read (in log order, partition by partition), how many of each topic, the records
/// passed over, and the topics that came and went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looked {
    pub hits: Vec<Hit>,
    pub scanned: Vec<(String, u64)>,
    pub skipped: u64,
    pub joined: Vec<TailTopic>,
    pub left: Vec<String>,
}

impl Looked {
    /// Counts one record read of `topic`.
    pub fn count(&mut self, topic: &str, n: u64) {
        match self.scanned.iter_mut().find(|(t, _)| t == topic) {
            Some((_, c)) => *c += n,
            None => self.scanned.push((topic.to_owned(), n)),
        }
    }
}

/// A topic a tail follows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailTopic {
    pub name: String,
    pub partitions: i32,
}

/// The first event: what is followed and from where.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicsTailReady {
    pub topics: Vec<TailTopic>,
    pub cursor: String,
    /// What the page says about the ask (a pattern that matched nothing yet, topics left out past the limits).
    pub notes: Vec<String>,
    pub pace_secs: u64,
    pub most_topics: usize,
    pub most_partitions: usize,
}

/// What one look read of one topic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailCount {
    pub topic: String,
    pub scanned: u64,
    pub matched: u64,
}

/// A record of the tail: where it is, its key and value as the Messages table shows them, its first headers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailRecord {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub timestamp_ms: i64,
    pub key: Shown,
    pub value: Shown,
    pub headers: Vec<HeaderRow>,
    /// How many headers the record has (more than `headers` holds past [`EVENT_HEADERS`]).
    pub headers_total: u32,
    pub standing: Standing,
}

/// An event of the tail: a look that read something, or found the topics changed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicsTailBatch {
    /// The records the filters kept, newest first, at most [`EVENT_ROWS`].
    pub rows: Vec<TailRecord>,
    /// Records read, and kept, by this look.
    pub scanned: u64,
    pub matched: u64,
    pub counts: Vec<TailCount>,
    /// Records passed over by partitions too far behind.
    pub skipped: u64,
    pub joined: Vec<TailTopic>,
    pub left: Vec<String>,
    pub cursor: String,
    pub at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Vec<(String, String)> {
        crate::form::parse(s)
    }

    #[test]
    fn an_ask_reads_names_patterns_start_partitions_and_filters() {
        let a = Ask::parse(
            &q("topic=orders&topic=orders&pattern=pay.*&from=last&last=900&partition=1&partition=0&key=ord&field=a.b&equals=7&header_value=x"),
            |n| !n.contains(' '),
        )
        .unwrap();
        assert_eq!(a.names, ["orders"]);
        assert_eq!(a.patterns, ["pay.*"]);
        assert_eq!(a.start, Start::Last(MOST_LAST));
        assert_eq!(a.partitions, [0, 1]);
        assert_eq!(a.filter.key.as_deref(), Some("ord"));
        assert_eq!(a.filter.equals.as_deref(), Some("7"));
        assert_eq!(
            a.filter.header_value, None,
            "a header value needs a header name"
        );
        assert!(
            a.wants("orders", true)
                && a.wants("pay.eu", false)
                && !a.wants("pay.eu", true)
                && !a.wants("x", false)
        );
        assert_eq!(
            Ask::parse(&q("topic=a"), |_| true).unwrap().start,
            Start::Now
        );
        for (bad, words) in [
            ("", "Name a topic"),
            ("pattern=a%20b", "pattern"),
            ("topic=a&field=a..b", "JSON field"),
            ("topic=a%20b", "cannot be"),
        ] {
            let e = Ask::parse(&q(bad), |n| !n.contains(' ')).unwrap_err();
            assert!(e.contains(words), "{bad}: {e}");
        }
    }

    #[test]
    fn a_cursor_names_every_partition_and_reads_back() {
        let place = |t: &str, p, o| Place {
            topic: t.into(),
            id: Uuid::nil(),
            partition: p,
            next: o,
            end: o,
        };
        let mut f = Follow::new(Ask::parse(&q("topic=a"), |_| true).unwrap(), None);
        f.places = vec![
            place("orders", 0, 12),
            place("orders", 1, 40),
            place("pay.eu", 0, 3),
        ];
        let c = f.cursor();
        assert_eq!(c, "orders:0=12,1=40;pay.eu:0=3");
        assert_eq!(
            decode(&c).unwrap(),
            [
                ("orders".into(), 0, 12),
                ("orders".into(), 1, 40),
                ("pay.eu".into(), 0, 3)
            ]
        );
        for bad in ["", "orders", "orders:0", "orders:x=1", "orders:0=y"] {
            assert_eq!(decode(bad), None, "{bad}");
        }
        assert!(Follow::new(f.ask.clone(), Some(&c)).resume.is_some());
    }
}
