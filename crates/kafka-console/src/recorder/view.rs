//! The recorder's answers to the pages: models, never ring types.

use std::collections::BTreeMap;

use super::errors::{self, Class};
use super::phrase;
use super::row::Row;
use super::{Recorder, names, ring};
use crate::model::live::{
    LiveState, LiveView, WireClass, WireQuery, WireRefusal, WireRow, WireWindow,
};

/// The requests in a minute above which the page is "a busy minute".
const BUSY: u32 = 100;
/// Refusals linked from the top of the tape.
const REFUSED_LINKS: usize = 6;

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
    }
}

fn has(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// Whether a row passes the person's filters.
fn matches(row: &Row, q: &WireQuery) -> bool {
    let api = names::api(row.api);
    let in_topics = |n: &str| row.facts.topics.iter().any(|t| has(&t.name, n));
    let in_group = |n: &str| row.facts.group.as_deref().is_some_and(|g| has(g, n));
    (q.client.is_empty() || has(&row.who.client, &q.client))
        && (q.api.is_empty() || api.eq_ignore_ascii_case(&q.api))
        && (q.topic.is_empty() || in_topics(&q.topic))
        && (q.group.is_empty() || in_group(&q.group))
        && (q.text.is_empty()
            || has(&row.who.client, &q.text)
            || has(api, &q.text)
            || in_topics(&q.text)
            || in_group(&q.text))
        && (!q.refused_only || class_of(row).is_refusal())
        && (!q.hide_idle || !row.idle)
        && q.before.is_none_or(|b| row.id < b)
        && q.until.is_none_or(|u| row.id <= u)
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
        let held = q.until.map(|u| {
            ring.iter()
                .rev()
                .take_while(|r| r.id > u)
                .map(|r| u64::from(r.count))
                .sum()
        });
        let mut clients: Vec<&str> = ring.iter().map(|r| &*r.who.client).collect();
        clients.sort_unstable();
        clients.dedup();
        let state = state_of(ring, last_minute);
        LiveView {
            state,
            rows,
            matched,
            window: window(ring),
            query: q.clone(),
            refused: refusals(ring),
            refused_total: count(Class::Refused),
            denied_total: count(Class::Denied),
            clients: inner.conns.len(),
            last_minute,
            busiest,
            seen: ring.requests(),
            newest_id: ring.newest_id(),
            now_ms,
            listen: String::new(),
            started_ms: None,
            older,
            held,
            paused_at_ms: q.until.and_then(|u| ring.get(u).map(|r| r.last_ms)),
            clients_ids: clients.iter().take(50).map(ToString::to_string).collect(),
            topics_seen: topics_seen(ring),
        }
    }

    /// Rows that changed after `cursor` (newest first, at most a hundred) and the cursor to ask from next time.
    #[must_use]
    pub fn since(&self, cursor: u64) -> (u64, Vec<WireRow>) {
        let inner = self.lock();
        let rows = inner
            .ring
            .iter()
            .rev()
            .filter(|r| r.touched > cursor)
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

fn topics_seen(ring: &ring::Ring) -> Vec<String> {
    let mut t: Vec<&str> = ring
        .iter()
        .flat_map(|r| r.facts.topics.iter().map(|t| &*t.name))
        .collect();
    t.sort_unstable();
    t.dedup();
    t.into_iter().take(50).map(ToString::to_string).collect()
}
/// The newest rows that pass the filters, up to the limit; how many pass; and where the next page back begins.
fn page(ring: &ring::Ring, q: &WireQuery) -> (Vec<WireRow>, usize, Option<u64>) {
    let mut rows = Vec::new();
    let mut matched = 0usize;
    let mut older = None;
    for row in ring.iter().rev().filter(|r| matches(r, q)) {
        matched += 1;
        if rows.len() < q.limit {
            rows.push(wire(row));
        } else if older.is_none() {
            older = rows.last().map(|r| r.id);
        }
    }
    (rows, matched, older)
}

/// The newest refusals (Kafka's and the ACLs'), to link from the top of the tape.
fn refusals(ring: &ring::Ring) -> Vec<WireRefusal> {
    ring.iter()
        .rev()
        .filter(|r| matches!(class_of(r), WireClass::Refused | WireClass::Denied))
        .take(REFUSED_LINKS)
        .map(|r| WireRefusal {
            id: r.id,
            code_name: errors::name(r.code),
            code: r.code,
            client: r.who.client.to_string(),
            at_ms: r.last_ms,
            class: class_of(r),
        })
        .collect()
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
