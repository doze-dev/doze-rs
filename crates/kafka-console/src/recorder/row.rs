//! One row of the ring, and what a request carries between `begin` and `end`.

use std::mem::size_of;
use std::sync::Arc;

use kafka_wire::messages::ApiKey;

use super::errors::Class;
use super::facts::Facts;

/// Who a request came from, shared by the rows of a connection while nothing about it changes.
#[derive(Debug, PartialEq, Eq)]
pub struct Who {
    pub client: Box<str>,
    /// What ApiVersions said: the software and its version.
    pub software: Option<Box<str>>,
    pub version: Option<Box<str>>,
    pub principal: Box<str>,
    pub peer: Option<Box<str>>,
}

impl Who {
    fn weight(&self) -> usize {
        size_of::<Self>()
            + self.client.len()
            + self.software.as_ref().map_or(0, |s| s.len())
            + self.version.as_ref().map_or(0, |s| s.len())
            + self.principal.len()
            + self.peer.as_ref().map_or(0, |s| s.len())
    }
}

/// The most took samples a folded row keeps.
pub const SAMPLES: usize = 64;

/// A row: one request, or several of the same kind folded together.
#[derive(Debug)]
pub struct Row {
    pub id: u64,
    /// The version of the ring when the row last changed (a fold bumps it).
    pub touched: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub count: u32,
    pub conn: u64,
    pub who: Arc<Who>,
    pub api: ApiKey,
    pub version: i16,
    pub correlation: i32,
    pub facts: Box<Facts>,
    pub class: Class,
    /// Set when the connection was closed instead of answering: the server's words for it.
    pub closed: Option<Box<str>>,
    pub code: i16,
    pub request_bytes: u32,
    pub response_bytes: u32,
    /// How long each of the last [`SAMPLES`] requests took, in microseconds; the newest last.
    pub tooks: Vec<u32>,
    /// A poll that found nothing.
    pub idle: bool,
    /// Rows with the same non-zero key may be folded; zero never folds.
    pub fold: u64,
}

impl Row {
    /// About how many bytes the row holds: the struct, its text and its vectors. Clipped text bounds it; a test checks it.
    #[must_use]
    pub fn weight(&self) -> usize {
        let f = &*self.facts;
        let text = |o: &Option<Box<str>>| o.as_ref().map_or(0, |s| s.len());
        size_of::<Self>()
            + size_of::<Facts>()
            + self.who.weight()
            + self.closed.as_ref().map_or(0, |s| s.len())
            + self.tooks.capacity() * size_of::<u32>()
            + f.topics
                .iter()
                .map(|t| size_of_val(t) + t.name.len() + t.partitions.capacity() * 4)
                .sum::<usize>()
            + f.results
                .iter()
                .map(|r| size_of_val(r) + r.topic.len())
                .sum::<usize>()
            + [
                &f.group,
                &f.member,
                &f.txn_id,
                &f.reason,
                &f.what,
                &f.note,
                &f.record_note,
            ]
            .into_iter()
            .map(text)
            .sum::<usize>()
    }

    /// The median took, in microseconds, of the samples kept.
    #[must_use]
    pub fn median(&self) -> u32 {
        let mut s = self.tooks.clone();
        s.sort_unstable();
        s.get(s.len() / 2).copied().unwrap_or(0)
    }
}

/// What the recorder keeps from `begin` until `end`.
#[derive(Debug)]
pub struct Begun {
    pub conn: u64,
    pub who: Arc<Who>,
    pub api: ApiKey,
    pub version: i16,
    pub correlation: i32,
    pub facts: Facts,
    pub request_bytes: usize,
    pub at_ms: i64,
}
