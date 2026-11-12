//! The request recorder: what the broker has been asked, kept in a bounded ring, read by the Live page, the request drawers and
//! the docks.
//!
//! It is a [`kafka_server::Observer`]: the server tells it of each request twice (before the broker handles it and after) and of
//! each connection opening and closing. It keeps a small, clipped, secret-free summary ([`summary`]) of each request and its
//! answer in a ring bounded by rows AND bytes, folds repeats ([`ring`]), and keeps a table of the connections that are open. It wakes nobody:
//! a tail asks it what is new once a second (`Recorder::since`). Pages never see the ring's types: they ask for models
//! ([`crate::model::live`]) through [`Recorder::tape`], [`Recorder::get`], [`Recorder::since`] and the like.
//!
//! The honest window is computed from the oldest row, never promised. Idle, nothing is held.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kafka_server::{Answer, Asked, Call, ConnId, Observed, Observer, Opened, Pending};
use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

pub mod clients;
pub mod clip;
mod detail;
pub mod errors;
pub mod facts;
pub mod names;
mod others;
pub mod phrase;
pub mod ring;
pub mod row;
pub mod summary;
mod view;

use clients::Conns;
use facts::Facts;
use ring::{Fold, Ring};
use row::{Begun, Row};

/// The most topic ids the recorder remembers names for.
const MOST_IDS: usize = 2048;

#[derive(Debug, Default)]
struct Inner {
    ring: Ring,
    conns: Conns,
    /// Topic id (as text) to topic name, learned from the Metadata and CreateTopics answers that pass: Fetch names its topics by
    /// id from version 13.
    ids: HashMap<Box<str>, Box<str>>,
}

/// The recorder.
#[derive(Default)]
pub struct Recorder {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Recorder {
    /// Counts only: nothing a request carried is printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.lock();
        f.debug_struct("Recorder")
            .field("rows", &inner.ring.len())
            .field("bytes", &inner.ring.bytes())
            .field("connections", &inner.conns.len())
            .finish()
    }
}

impl Recorder {
    /// A recorder with nothing in it and no memory held.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The most bytes the ring has held (a test and the report read it).
    #[must_use]
    pub fn high_water(&self) -> usize {
        self.lock().ring.high_water()
    }

    /// How many rows and bytes it holds now.
    #[must_use]
    pub fn held(&self) -> (usize, usize) {
        let inner = self.lock();
        (inner.ring.len(), inner.ring.bytes())
    }
}

/// Milliseconds since the epoch.
fn now_ms() -> i64 {
    crate::source::now_ms()
}

impl Observer for Recorder {
    fn opened(&self, conn: &Opened) {
        self.lock().conns.open(conn.id.0, conn.peer, now_ms());
    }

    fn begin(&self, call: &Call<'_>) -> Pending {
        let at_ms = now_ms();
        let mut facts = Facts::default();
        let mut software = None;
        match &call.asked {
            Asked::Request(r) => summary::of_request(r, &mut facts),
            Asked::ApiVersions(body) => {
                if let Some(b) = body {
                    software = Some((
                        clip::name(&b.client_software_name),
                        clip::name(&b.client_software_version),
                    ));
                    summary::of_api_versions(b, &mut facts);
                }
            }
            Asked::Undecoded => {}
        }
        let client = call.head.client_id.as_deref().unwrap_or("");
        let who = self
            .lock()
            .conns
            .who(call.conn.0, client, call.principal.as_str(), software);
        Pending::new(Begun {
            conn: call.conn.0,
            who,
            api: call.head.key,
            version: call.head.version,
            correlation: call.head.correlation_id,
            facts,
            request_bytes: call.request_bytes,
            at_ms,
        })
    }

    fn end(&self, pending: Pending, seen: &Observed<'_>) {
        let Some(begun) = pending.into_payload::<Begun>() else {
            return;
        };
        let took = u32::try_from(seen.took.as_micros()).unwrap_or(u32::MAX);
        let now = now_ms();
        let mut inner = self.lock();
        let row = finish(&mut inner, begun, seen, took);
        let (errored, who_conn) = (
            row.class != errors::Class::Ok || row.closed.is_some(),
            row.conn,
        );
        let fold = fold_of(&row);
        let (api, version) = (row.api, row.version);
        inner.conns.note(who_conn, api, version, errored, now);
        inner.ring.record(row, fold);
        drop(inner);
    }

    fn closed(&self, conn: ConnId, _why: &str) {
        self.lock().conns.close(conn.0);
    }
}

/// Builds the row of a request that has ended, and learns what the answer teaches.
fn finish(inner: &mut Inner, begun: Begun, seen: &Observed<'_>, took: u32) -> Row {
    let mut facts = begun.facts;
    let mut closed = None;
    match &seen.answer {
        Answer::Response(r) => {
            summary::of_response(r, &mut facts);
            learn(&mut inner.ids, r);
        }
        Answer::Nothing => {}
        Answer::Closed(why) => closed = Some(clip::clip(why, clip::TEXT)),
    }
    resolve(&inner.ids, &mut facts);
    let code = facts.code;
    let class = if closed.is_some() {
        errors::Class::Bad
    } else {
        errors::class(code)
    };
    let idle = closed.is_none()
        && class == errors::Class::Ok
        && match begun.api {
            ApiKey::Heartbeat | ApiKey::ConsumerGroupHeartbeat => true,
            ApiKey::Fetch => facts.records == 0 && facts.batches == 0,
            _ => false,
        };
    let mut row = Row {
        id: 0,
        touched: 0,
        first_ms: begun.at_ms,
        last_ms: begun.at_ms,
        count: 1,
        conn: begun.conn,
        who: begun.who,
        api: begun.api,
        version: begun.version,
        correlation: begun.correlation,
        facts: Box::new(facts),
        class,
        closed,
        code,
        request_bytes: u32::try_from(begun.request_bytes).unwrap_or(u32::MAX),
        response_bytes: u32::try_from(seen.response_bytes).unwrap_or(u32::MAX),
        tooks: vec![took],
        idle,
        fold: 0,
        lab: seen.fault.map(|n| {
            Box::new(row::LabTag {
                rule: n.rule,
                name: clip::name(&n.name),
                applied: n.applied,
            })
        }),
    };
    row.fold = fold_key(&row);
    row
}

/// What an answer teaches: the names behind topic ids.
fn learn(ids: &mut HashMap<Box<str>, Box<str>>, r: &Response) {
    let mut put = |id: String, name: &str| {
        if ids.len() < MOST_IDS || ids.contains_key(id.as_str()) {
            ids.insert(id.into_boxed_str(), clip::name(name));
        }
    };
    match r {
        Response::Metadata(m) => {
            for t in &m.topics {
                if let Some(n) = &t.name {
                    put(t.topic_id.to_string(), &n.0);
                }
            }
        }
        Response::CreateTopics(c) => {
            for t in &c.topics {
                put(t.topic_id.to_string(), &t.name.0);
            }
        }
        _ => {}
    }
}

/// Puts the name of a topic in place of the id Fetch used, where it is known.
fn resolve(ids: &HashMap<Box<str>, Box<str>>, f: &mut Facts) {
    let name_of = |n: &str| (n.len() == 36).then(|| ids.get(n)).flatten().cloned();
    for t in &mut f.topics {
        if let Some(n) = name_of(&t.name) {
            t.name = n;
        }
    }
    for r in &mut f.results {
        if let Some(n) = name_of(&r.topic) {
            r.topic = n;
        }
    }
}

/// How a row may fold.
fn fold_of(row: &Row) -> Fold {
    if row.closed.is_some()
        || row.lab.is_some()
        || row.class != errors::Class::Ok
        || row.fold == 0
    {
        return Fold::Never;
    }
    match row.api {
        ApiKey::Heartbeat | ApiKey::ConsumerGroupHeartbeat => Fold::Poll,
        ApiKey::Fetch if row.idle => Fold::Poll,
        ApiKey::Produce
        | ApiKey::Fetch
        | ApiKey::OffsetCommit
        | ApiKey::OffsetFetch
        | ApiKey::ListOffsets
        | ApiKey::Metadata
        | ApiKey::FindCoordinator
        | ApiKey::TxnOffsetCommit
        | ApiKey::DescribeGroups
        | ApiKey::ListGroups => Fold::Stream,
        _ => Fold::Never,
    }
}

/// The key two rows share when they are the same kind of request: connection, API, version, target, result.
fn fold_key(row: &Row) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        row.conn,
        row.api as i16,
        row.version,
        row.code,
        row.facts.transactional,
    )
        .hash(&mut h);
    let f = &*row.facts;
    for t in &f.topics {
        (&t.name, &t.partitions).hash(&mut h);
    }
    (&f.group, &f.member, &f.txn_id, &f.what).hash(&mut h);
    h.finish().max(1)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_facets;
