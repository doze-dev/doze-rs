//! What a person asked of the Messages page, validated: the filters in the address, as values. Nothing here reaches
//! the broker unchecked, and a value that cannot be used is a sentence on the page, never an error.

use crate::format::parse_time;

/// How many records a page shows unless asked.
pub const DEFAULT_LIMIT: usize = 25;
/// The most it shows.
pub const MOST_LIMIT: usize = 200;
/// The longest key searched for, in bytes.
pub const MOST_KEY: usize = 512;

/// Which isolation level the table reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Uncommitted,
    Committed,
}

impl Reading {
    /// The word the page and the address use.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Uncommitted => "read_uncommitted",
            Self::Committed => "read_committed",
        }
    }
}

/// The filters of the Messages page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageQuery {
    pub partition: Option<i32>,
    /// From this offset (on the chosen partition).
    pub offset: Option<i64>,
    /// From this time, in milliseconds since the epoch, and as it was typed.
    pub time: Option<(i64, String)>,
    /// Records whose key is exactly this text.
    pub key: Option<String>,
    pub reading: Reading,
    pub limit: usize,
}

impl Default for MessageQuery {
    fn default() -> Self {
        Self {
            partition: None,
            offset: None,
            time: None,
            key: None,
            reading: Reading::Uncommitted,
            limit: DEFAULT_LIMIT,
        }
    }
}

/// The first value of a field of the address that is not blank.
fn get<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.trim().is_empty())
}

fn whole(text: &str) -> Option<i64> {
    let t = text.trim();
    (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .then(|| t.parse::<i64>().ok())
        .flatten()
}

impl MessageQuery {
    /// Whether any filter besides the isolation level is set.
    #[must_use]
    pub fn filtered(&self) -> bool {
        self.partition.is_some()
            || self.offset.is_some()
            || self.time.is_some()
            || self.key.is_some()
    }

    /// The query from the fields of an address or a form, and a sentence for each value that could not be used.
    #[must_use]
    pub fn parse(fields: &[(String, String)]) -> (Self, Vec<String>) {
        let mut q = Self::default();
        let mut notes = Vec::new();
        if let Some(p) = get(fields, "partition") {
            match p.trim().parse::<i32>().ok().filter(|n| *n >= 0) {
                Some(n) => q.partition = Some(n),
                None => notes.push(format!(
                    "{:?} is not a partition number, so all partitions are shown.",
                    clip(p)
                )),
            }
        }
        // The one box: digits are an offset, anything else a key.
        let find = get(fields, "find");
        let (find_offset, find_key) = match find {
            Some(f) if whole(f).is_some() => (Some(f), None),
            other => (None, other),
        };
        if let Some(o) = get(fields, "offset").or(find_offset) {
            match whole(o) {
                Some(n) => q.offset = Some(n),
                None => notes.push(format!(
                    "{:?} is not an offset (a whole number from 0), so it is ignored.",
                    clip(o)
                )),
            }
        }
        if let Some(t) = get(fields, "time") {
            match parse_time(t) {
                Some(ms) => q.time = Some((ms, t.trim().to_owned())),
                None => notes.push(format!(
                    "{:?} is not a time. Write milliseconds since the epoch, or a UTC time like 2026-11-09 22:13.",
                    clip(t)
                )),
            }
        }
        q.parse_rest(fields, find_key, &mut notes);
        (q, notes)
    }

    /// The key, the isolation level and the limit.
    fn parse_rest(
        &mut self,
        fields: &[(String, String)],
        find_key: Option<&str>,
        notes: &mut Vec<String>,
    ) {
        let q = self;
        if let Some(k) = get(fields, "key").or(find_key) {
            if k.len() > MOST_KEY {
                notes.push(format!("A key to look for is at most {MOST_KEY} bytes; this one is {}, so it is ignored.", k.len()));
            } else {
                q.key = Some(k.to_owned());
            }
        }
        match get(fields, "isolation") {
            None | Some("read_uncommitted") => {}
            Some("read_committed") => q.reading = Reading::Committed,
            Some(other) => notes.push(format!(
                "{:?} is not an isolation level; the choices are read_uncommitted and read_committed.",
                clip(other)
            )),
        }
        if let Some(l) = get(fields, "limit") {
            match l.trim().parse::<usize>().ok().filter(|n| *n >= 1) {
                Some(n) if n > MOST_LIMIT => {
                    q.limit = MOST_LIMIT;
                    notes.push(format!(
                        "A page shows at most {MOST_LIMIT} records; showing {MOST_LIMIT}."
                    ));
                }
                Some(n) => q.limit = n,
                None => notes.push(format!(
                    "{:?} is not a number of records, so {DEFAULT_LIMIT} are shown.",
                    clip(l)
                )),
            }
        }
    }

    /// The address's query part (with the leading `?`, or nothing) that gives this query back.
    #[must_use]
    pub fn to_query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = self.partition {
            parts.push(format!("partition={p}"));
        }
        if let Some(o) = self.offset {
            parts.push(format!("offset={o}"));
        }
        if let Some((_, text)) = &self.time {
            parts.push(format!("time={}", crate::base::segment(text)));
        }
        if let Some(k) = &self.key {
            parts.push(format!("key={}", crate::base::segment(k)));
        }
        if self.reading == Reading::Committed {
            parts.push("isolation=read_committed".to_owned());
        }
        if self.limit != DEFAULT_LIMIT {
            parts.push(format!("limit={}", self.limit));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// A value of the address, shortened for a sentence.
fn clip(s: &str) -> String {
    s.chars().take(40).collect()
}

#[cfg(test)]
mod tests;
