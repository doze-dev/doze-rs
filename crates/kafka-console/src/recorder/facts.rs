//! What the recorder keeps of one request: small, clipped and secret-free. Built in two steps by `summary`: from the request
//! (before the broker handles it) and from the answer (after).
//!
//! Never in here: record keys, values or headers, SASL tokens, SCRAM nonces, proofs, salts or salted passwords, config
//! values, ACL principals' credentials. A field exists only if a page shows it.

use super::clip;

/// The most topics one request keeps by name; the rest are counted.
pub const TOPICS: usize = 20;
/// The most partitions kept for one topic.
pub const PARTITIONS: usize = 8;
/// The most per-partition results kept; the rest are counted.
pub const RESULTS: usize = 16;

/// A topic a request named, and some of its partitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicFact {
    pub name: Box<str>,
    pub partitions: Vec<i32>,
    /// More partitions than were kept.
    pub more: u32,
}

/// One per-partition result of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartResult {
    pub topic: Box<str>,
    pub partition: i32,
    pub code: i16,
    /// The base offset of a Produce, the committed or returned offset of the APIs that have one.
    pub offset: Option<i64>,
    pub records: u32,
    pub bytes: u64,
}

/// Everything a row keeps besides who, when and how long.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub topics: Vec<TopicFact>,
    /// Topics past [`TOPICS`].
    pub topics_more: u32,
    pub group: Option<Box<str>>,
    pub member: Option<Box<str>>,
    pub generation: Option<i32>,
    pub txn_id: Option<Box<str>>,
    /// Producer id and epoch, as the request or the answer carried them.
    pub producer: Option<(i64, i16)>,
    /// A Produce that is part of a transaction (its batch says so).
    pub transactional: bool,
    pub acks: Option<i16>,
    /// A Fetch's first offset (of its first partition), a ListOffsets timestamp is not kept.
    pub from_offset: Option<i64>,
    /// How long a Fetch was willing to wait.
    pub wait_ms: Option<i32>,
    pub isolation: Option<i8>,
    /// The client's own reason (JoinGroup, LeaveGroup), clipped.
    pub reason: Option<Box<str>>,
    /// What the request is about when it names no topic or group: `all topics`, a mechanism, a user.
    pub what: Option<Box<str>>,
    /// The broker's own message, if its answer carried one.
    pub note: Option<Box<str>>,
    pub records: u32,
    pub batches: u32,
    /// Record bytes in or out (the size of the batches, never their content).
    pub bytes: u64,
    /// How many of something the answer holds: topics listed, groups, members, ACLs.
    pub items: u32,
    /// How many the request asked for, where that differs from what came back.
    pub asked: u32,
    pub results: Vec<PartResult>,
    pub results_more: u32,
    /// The first error code of the answer that is not zero; zero when every part succeeded.
    pub code: i16,
    /// How many parts of the answer failed.
    pub failed: u32,
    /// A message that came with a record error (the `INVALID_RECORD` case), clipped.
    pub record_note: Option<Box<str>>,
}

impl Facts {
    /// Names a topic with some of its partitions.
    pub fn topic(&mut self, name: &str, partitions: impl IntoIterator<Item = i32>) {
        if self.topics.iter().any(|t| &*t.name == name) {
            return;
        }
        if self.topics.len() >= TOPICS {
            self.topics_more = self.topics_more.saturating_add(1);
            return;
        }
        let mut kept = Vec::new();
        let mut more = 0u32;
        for p in partitions {
            if kept.len() < PARTITIONS {
                kept.push(p);
            } else {
                more = more.saturating_add(1);
            }
        }
        self.topics.push(TopicFact {
            name: clip::name(name),
            partitions: kept,
            more,
        });
    }

    /// Sets the group, clipped.
    pub fn group(&mut self, group: &str) {
        if !group.is_empty() {
            self.group = Some(clip::name(group));
        }
    }

    /// Sets the member id, clipped.
    pub fn member(&mut self, member: &str) {
        if !member.is_empty() {
            self.member = Some(clip::name(member));
        }
    }

    /// Sets the transactional id, clipped.
    pub fn txn(&mut self, id: &str) {
        if !id.is_empty() {
            self.txn_id = Some(clip::name(id));
        }
    }

    /// Sets what the request is about.
    pub fn about(&mut self, what: &str) {
        self.what = clip::text(what);
    }

    /// Keeps the broker's own message, clipped.
    pub fn say(&mut self, note: Option<&str>) {
        if self.note.is_none() {
            self.note = note.and_then(clip::text);
        }
    }

    /// Notes one result: the first non-zero code is the row's, and every failure is counted.
    pub fn result(&mut self, topic: &str, partition: i32, code: i16, offset: Option<i64>) {
        self.part(PartResult {
            topic: clip::name(topic),
            partition,
            code,
            offset,
            records: 0,
            bytes: 0,
        });
    }

    /// Notes one fully described result.
    pub fn part(&mut self, r: PartResult) {
        self.error(r.code);
        if self.results.len() < RESULTS {
            self.results.push(r);
        } else {
            self.results_more = self.results_more.saturating_add(1);
        }
    }

    /// Notes an error code of the answer (the whole answer's, or one item's).
    pub fn error(&mut self, code: i16) {
        if code != 0 {
            self.failed = self.failed.saturating_add(1);
            if self.code == 0 {
                self.code = code;
            }
        }
    }

    /// A short canonical text of the facts, for the golden files.
    #[must_use]
    pub fn canon(&self) -> String {
        format!("{self:#?}")
    }
}

/// Counts the records and batches in a Produce or Fetch payload from its batch headers alone: never a copy, never a read of
/// a record. At most 64 batches are walked.
#[must_use]
pub fn batches(bytes: &[u8]) -> BatchSummary {
    const HEADER: usize = 61;
    let mut out = BatchSummary::default();
    let mut at = 0usize;
    while out.batches < 64 {
        let Some(head) = bytes.get(at..at + HEADER) else {
            break;
        };
        let len = head
            .get(8..12)
            .and_then(|b| b.try_into().ok())
            .map_or(0, i32::from_be_bytes);
        let count = head
            .get(57..61)
            .and_then(|b| b.try_into().ok())
            .map_or(0, i32::from_be_bytes);
        out.records = out
            .records
            .saturating_add(u32::try_from(count.max(0)).unwrap_or(0));
        out.batches += 1;
        if out.batches == 1 {
            out.transactional = head
                .get(21..23)
                .and_then(|b| b.try_into().ok())
                .map(u16::from_be_bytes)
                .is_some_and(|a| a & 0x10 != 0);
            let pid = head
                .get(43..51)
                .and_then(|b| b.try_into().ok())
                .map_or(-1, i64::from_be_bytes);
            let epoch = head
                .get(51..53)
                .and_then(|b| b.try_into().ok())
                .map_or(-1, i16::from_be_bytes);
            out.producer = (pid >= 0).then_some((pid, epoch));
        }
        let Some(next) = usize::try_from(len)
            .ok()
            .and_then(|l| at.checked_add(12 + l))
        else {
            break;
        };
        if next <= at {
            break;
        }
        at = next;
    }
    out.bytes = bytes.len() as u64;
    out
}

/// What the batch headers of a payload say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchSummary {
    pub batches: u32,
    pub records: u32,
    pub bytes: u64,
    pub transactional: bool,
    /// The producer id and epoch of the first batch, when it has one.
    pub producer: Option<(i64, i16)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_are_bounded_clipped_and_counted_past_the_bound() {
        let mut f = Facts::default();
        for i in 0..30 {
            f.topic(&format!("t{i}"), 0..20);
        }
        assert_eq!(f.topics.len(), TOPICS);
        assert_eq!(f.topics_more, 10);
        assert_eq!(f.topics[0].partitions.len(), PARTITIONS);
        assert_eq!(f.topics[0].more, 12);
        f.topic("t0", [1]);
        assert_eq!(f.topics.len(), TOPICS, "a topic named twice is kept once");
        let mut g = Facts::default();
        g.topic(&"x".repeat(5000), []);
        assert!(g.topics[0].name.len() <= clip::NAME);
    }

    #[test]
    fn the_first_error_is_the_rows_and_every_failure_is_counted() {
        let mut f = Facts::default();
        f.result("a", 0, 0, Some(5));
        f.result("a", 1, 10, None);
        f.result("a", 2, 3, None);
        assert_eq!((f.code, f.failed, f.results.len()), (10, 2, 3));
        for i in 0..40 {
            f.result("b", i, 0, None);
        }
        assert_eq!(f.results.len(), RESULTS);
        assert_eq!(f.results_more, 43 - RESULTS as u32);
    }

    #[test]
    fn batch_headers_are_walked_not_records() {
        use kafka_wire::batch::build::BatchBuilder;
        let one = BatchBuilder::new()
            .records(3)
            .producer(42, 7, 0)
            .attributes(0x10)
            .body(vec![1, 2, 3])
            .build();
        let two = BatchBuilder::new().records(2).build();
        let both = [one.as_ref(), two.as_ref()].concat();
        let s = batches(&both);
        assert_eq!((s.batches, s.records, s.bytes), (2, 5, both.len() as u64));
        assert!(s.transactional);
        assert_eq!(s.producer, Some((42, 7)));
        assert_eq!(batches(&[]), BatchSummary::default());
        assert_eq!(batches(&[0; 30]).batches, 0);
        // A header that claims a length that overruns ends the walk.
        let mut bad = one.to_vec();
        bad[8..12].copy_from_slice(&i32::MAX.to_be_bytes());
        assert_eq!(batches(&bad).batches, 1);
    }
}
