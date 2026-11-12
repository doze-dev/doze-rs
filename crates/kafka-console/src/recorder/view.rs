//! The recorder's answers to the pages: models, never ring types.

use std::collections::BTreeMap;

use super::errors::{self, Class};
use super::phrase;
use super::row::Row;
use super::{Recorder, names, ring};
use crate::model::live::{
    Facet, Facets, LiveState, LiveView, WireClass, WireQuery, WireRefusal, WireRow, WireWindow,
};

/// The requests in a minute above which the page is "a busy minute".
const BUSY: u32 = 100;
/// Refusals named at the top of the tape: one line for each (code, client), at most this many, then "and N more".
const REFUSED_LINES: usize = 3;

/// The class a row is shown with.
pub(super) fn class_of(row: &Row) -> WireClass {
    if row.closed.is_some() {
        return WireClass::Closed;
    }
    match row.class {
        Class::Ok => WireClass::Done,
        Class::Warn => WireClass::Told,
        Class::Bad => WireClass::Failed,
        Class::Refused => WireClass::Refused,
        Class::Denied => WireClass::Denied,
    }
}

/// A row as the tape shows it.
pub(super) fn wire(row: &Row) -> WireRow {
    wire_for(row, &WireQuery::default())
}

/// A row as the tape shows it for a person who filtered by topic: the topic it passed by leads (a Metadata that lists six topics,
/// a group's commit, a Produce to two), so that a filtered tape never shows rows that do not say what they were filtered by.
pub(super) fn wire_for(row: &Row, q: &WireQuery) -> WireRow {
    let mut w = plain(row);
    if q.topic.is_empty() {
        return w;
    }
    if let Some((target, sub)) = phrase::target_through(row, &q.topic) {
        w.target = target;
        w.target_sub = sub;
        // The row's cell links to its first topic: the one it passed by.
        let hit = w.topics.iter().position(|t| q.topic.contains(t));
        if let Some(i) = hit {
            w.topics.swap(0, i);
        }
    }
    w
}

fn plain(row: &Row) -> WireRow {
    let (target, target_sub) = phrase::target(row);
    let class = class_of(row);
    let detail = match class {
        WireClass::Done | WireClass::Told => phrase::done(row).unwrap_or_default(),
        WireClass::Closed => row.closed.as_deref().unwrap_or("").to_owned(),
        _ => phrase::refused(row),
    };
    WireRow {
        id: row.id,
        at_ms: row.last_ms,
        since_ms: (row.count > 1).then_some(row.first_ms),
        count: row.count,
        client: row.who.client.to_string(),
        tag: row.who.software.as_deref().and_then(names::tag),
        api: names::api(row.api),
        version: row.version,
        transactional: row.facts.transactional,
        target,
        target_sub,
        topics: row
            .facts
            .topics
            .iter()
            .map(|t| t.name.to_string())
            .collect(),
        group: row.facts.group.as_ref().map(ToString::to_string),
        class,
        code: row.code,
        code_name: errors::name(row.code),
        detail,
        took_us: if row.count > 1 {
            row.median()
        } else {
            row.tooks.last().copied().unwrap_or(0)
        },
        took_sub: phrase::took_sub(row),
        idle: row.idle,
        lab: row.lab.as_ref().map(|l| crate::model::live::LabTag {
            rule: l.rule,
            name: l.name.to_string(),
            what: match l.applied {
                kafka_server::fault::Applied::Delay(d) => crate::model::live::LabDid::Delayed(
                    u32::try_from(d.as_millis()).unwrap_or(u32::MAX),
                ),
                kafka_server::fault::Applied::Fail(_) => crate::model::live::LabDid::Failed,
                kafka_server::fault::Applied::Drop => crate::model::live::LabDid::Dropped,
            },
        }),
    }
}

/// Whether an API is of the kind a group wire chip names.
fn of_kind(api: kafka_wire::messages::ApiKey, kind: &str) -> bool {
    use kafka_wire::messages::ApiKey as K;
    match kind {
        "join" => matches!(api, K::JoinGroup | K::SyncGroup | K::LeaveGroup),
        "commits" => matches!(api, K::OffsetCommit | K::TxnOffsetCommit),
        "heartbeats" => matches!(api, K::Heartbeat | K::ConsumerGroupHeartbeat),
        _ => true,
    }
}

fn has(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// How a row stands against each kind of filter. A row is shown when all four hold; a filter's own options count the rows that
/// pass the other three (what choosing that option would leave).
#[derive(Clone, Copy)]
struct Verdict {
    client: bool,
    api: bool,
    topic: bool,
    rest: bool,
}

impl Verdict {
    fn all(self) -> bool {
        self.client && self.api && self.topic && self.rest
    }
}

fn judge(row: &Row, q: &WireQuery) -> Verdict {
    let api = names::api(row.api);
    let in_topics = |n: &str| row.facts.topics.iter().any(|t| has(&t.name, n));
    let in_group = |n: &str| row.facts.group.as_deref().is_some_and(|g| has(g, n));
    Verdict {
        client: q.client_ok(&row.who.client),
        api: q.api_ok(api),
        topic: q.topic_ok(row.facts.topics.iter().map(|t| &*t.name)),
        rest: (q.kind.is_empty() || of_kind(row.api, &q.kind))
            && (q.group.is_empty() || in_group(&q.group))
            && (q.text.is_empty()
                || has(&row.who.client, &q.text)
                || has(api, &q.text)
                || in_topics(&q.text)
                || in_group(&q.text))
            && (!q.refused_only || class_of(row).is_refusal())
            && (!q.hide_idle || !row.idle)
            && q.before.is_none_or(|b| row.id < b),
    }
}

/// The most options a filter lists.
const OPTIONS: usize = 50;

/// The options of the filters, each with the rows it would leave: names sorted, the busiest kept when there are more than fit.
fn facets(ring: &ring::Ring, q: &WireQuery) -> Facets {
    let mut clients: BTreeMap<&str, usize> = BTreeMap::new();
    let mut apis: BTreeMap<&str, usize> = BTreeMap::new();
    let mut topics: BTreeMap<&str, usize> = BTreeMap::new();
    for row in ring.iter() {
        // A page back is the page's own state, not a filter: the options always count the whole tape.
        let v = judge(
            row,
            &WireQuery {
                before: None,
                ..q.clone()
            },
        );
        if v.api && v.topic && v.rest {
            *clients.entry(&row.who.client).or_default() += 1;
        }
        if v.client && v.topic && v.rest {
            *apis.entry(names::api(row.api)).or_default() += 1;
        }
        if v.client && v.api && v.rest {
            let mut seen: Vec<&str> = Vec::new();
            for t in row.facts.topics.iter().map(|t| &*t.name) {
                if !seen.contains(&t) {
                    seen.push(t);
                    *topics.entry(t).or_default() += 1;
                }
            }
        }
    }
    let list = |m: BTreeMap<&str, usize>, chosen: &[String]| {
        let mut all: Vec<(&str, usize)> = m.into_iter().collect();
        if all.len() > OPTIONS {
            // Keep the chosen ones and the busiest of the rest.
            all.sort_by_key(|(n, c)| (!chosen.iter().any(|x| x == n), std::cmp::Reverse(*c)));
            all.truncate(OPTIONS);
            all.sort_unstable_by_key(|(n, _)| *n);
        }
        all.into_iter()
            .map(|(value, count)| Facet {
                value: value.to_owned(),
                count,
            })
            .collect()
    };
    Facets {
        clients: list(clients, &q.client),
        apis: list(apis, &q.api),
        topics: list(topics, &q.topic),
    }
}

/// Whether a row passes the person's filters.
fn matches(row: &Row, q: &WireQuery) -> bool {
    judge(row, q).all()
}

impl Recorder {
    /// The Live page's data: the newest rows that pass the filters, what the ring covers, and the counts around them.
    #[must_use]
    pub fn tape(&self, q: &WireQuery, now_ms: i64) -> LiveView {
        let inner = self.lock();
        let ring = &inner.ring;
        let (rows, matched, older) = page(ring, q);
        let count = |c: Class| {
            ring.iter()
                .filter(|r| r.closed.is_none() && r.class == c)
                .count()
        };
        let (last_minute, busiest) = last_minute(ring, now_ms);
        let state = state_of(ring, last_minute);
        LiveView {
            state,
            rows,
            matched,
            window: window(ring),
            query: q.clone(),
            refused: refusals(ring).0,
            refused_more: refusals(ring).1,
            refused_total: count(Class::Refused),
            denied_total: count(Class::Denied),
            clients: inner.conns.len(),
            last_minute,
            busiest,
            seen: ring.requests(),
            newest_id: ring.newest_id(),
            cursor: ring.version(),
            now_ms,
            listen: String::new(),
            started_ms: None,
            older,
            facets: facets(ring, q),
        }
    }

    /// Rows that passed the filters in `q` and changed after `cursor` (newest first, at most a hundred), and the cursor to ask from
    /// next time. A row that folded a repeat since is among them, with its count grown.
    #[must_use]
    pub fn since(&self, q: &WireQuery, cursor: u64) -> (u64, Vec<WireRow>) {
        let inner = self.lock();
        let rows = inner
            .ring
            .iter()
            .rev()
            .filter(|r| r.touched > cursor && matches(r, q))
            .take(100)
            .map(wire)
            .collect();
        (inner.ring.version(), rows)
    }
}

/// Requests in the last minute, and the topic that took most of them (by the requests its rows stand for).
fn last_minute(ring: &ring::Ring, now_ms: i64) -> (u32, Option<(String, u32)>) {
    let mut total = 0u32;
    let mut by: BTreeMap<&str, u32> = BTreeMap::new();
    for r in ring
        .iter()
        .rev()
        .take_while(|r| now_ms - r.last_ms <= 60_000)
    {
        total = total.saturating_add(r.count);
        if let Some(t) = r.facts.topics.first() {
            *by.entry(&t.name).or_default() += r.count;
        }
    }
    let busiest = by
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(t, n)| (t.to_owned(), n));
    (total, busiest)
}

fn window(ring: &ring::Ring) -> WireWindow {
    WireWindow {
        rows: ring.len(),
        capacity: ring::ROWS,
        oldest_ms: ring.iter().next().map(|r| r.first_ms),
        requests: ring.requests(),
    }
}

/// The newest rows that pass the filters, up to the limit; how many pass; and where the next page back begins.
fn page(ring: &ring::Ring, q: &WireQuery) -> (Vec<WireRow>, usize, Option<u64>) {
    let mut rows = Vec::new();
    let mut matched = 0usize;
    let mut older = None;
    for row in ring.iter().rev().filter(|r| matches(r, q)) {
        matched += 1;
        if rows.len() < q.limit {
            rows.push(wire_for(row, q));
        } else if older.is_none() {
            older = rows.last().map(|r| r.id);
        }
    }
    (rows, matched, older)
}

/// The refusals, folded: one entry for each (code, client) with how many there are and the newest, newest first, at most
/// [`REFUSED_LINES`]; and how many more pairs there are.
fn refusals(ring: &ring::Ring) -> (Vec<WireRefusal>, usize) {
    let mut by: Vec<WireRefusal> = Vec::new();
    for r in ring
        .iter()
        .rev()
        .filter(|r| matches!(class_of(r), WireClass::Refused | WireClass::Denied))
    {
        match by
            .iter_mut()
            .find(|x| x.code == r.code && *x.client == *r.who.client)
        {
            Some(x) => x.count = x.count.saturating_add(r.count),
            None => by.push(WireRefusal {
                id: r.id,
                count: r.count,
                code_name: errors::name(r.code),
                code: r.code,
                client: r.who.client.to_string(),
                at_ms: r.last_ms,
                class: class_of(r),
            }),
        }
    }
    let more = by.len().saturating_sub(REFUSED_LINES);
    by.truncate(REFUSED_LINES);
    (by, more)
}

fn state_of(ring: &ring::Ring, last_minute: u32) -> LiveState {
    if ring.requests() == 0 {
        LiveState::Quiet
    } else if last_minute >= BUSY && ring.iter().any(|r| r.count > 1) {
        LiveState::Busy
    } else {
        LiveState::Steady
    }
}
