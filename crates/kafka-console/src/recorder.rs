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
mod lab;
pub mod names;
mod others;
pub mod phrase;
pub mod ring;
pub mod row;
pub mod summary;
mod topics;
mod view;

use clients::Conns;
use facts::Facts;
use ring::{Fold, Repeat, Ring};
use row::{Begun, Row};

/// The most topic ids the recorder remembers names for.
const MOST_IDS: usize = 2048;

#[derive(Debug, Default)]
struct Inner {
    ring: Ring,
    conns: Conns,
    /// Topic id (as text) to topic name, learned from the Metadata and CreateTopics answers that pass: Fetch names its topics by
    /// id from version 13.
    ids: HashMap<Box<str>, Arc<str>>,
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
        })
    }

    fn end(&self, pending: Pending, seen: &Observed<'_>) {
        let Some(begun) = pending.into_payload::<Begun>() else {
            return;
        };
        // The answer is read before the lock is taken: only what needs the shared tables (the topic names, the connection,
        // the ring) is done under it.
        let mut a = answered(begun, seen);
        let mut inner = self.lock();
        // Stamped when it is answered, under the lock that adds it: a row's time is the time it joins the tape, so a slow
        // Fetch that began earlier is never shown below rows it was added above.
        let now = now_ms();
        if let Answer::Response(r) = &seen.answer {
            learn(&mut inner.ids, r);
        }
        resolve(&inner.ids, &mut a.facts);
        let b = &a.begun;
        let key = fold_key(b.conn, b.api, b.version, &a.facts);
        let fold = if a.closed.is_some() || a.lab.is_some() || a.class != errors::Class::Ok {
            Fold::Never
        } else {
            fold_of(b.api, a.idle, key)
        };
        let errored = a.class != errors::Class::Ok || a.closed.is_some();
        inner.conns.note(b.conn, b.api, b.version, errored, now);
        // A repeat folds into the row it repeats before a row is built for it: it costs no row, no boxed facts and no list
        // of tooks, only the counts it adds.
        let repeat = Repeat {
            at_ms: now,
            last_ms: now,
            count: 1,
            correlation: b.correlation,
            request_bytes: a.request_bytes,
            response_bytes: a.response_bytes,
            records: a.facts.records,
            batches: a.facts.batches,
            bytes: a.facts.bytes,
            from_offset: a.facts.from_offset,
            tooks: &[a.took],
        };
        let spare = if inner.ring.fold_in(key, fold, &repeat).is_some() {
            Some(a)
        } else {
            inner.ring.push(a.row(now, key), fold);
            None
        };
        drop(inner);
        // What a repeat carried is freed after the lock is given back.
        drop(spare);
    }

    fn closed(&self, conn: ConnId, _why: &str) {
        self.lock().conns.close(conn.0);
    }
}

/// A request and what its answer says, before it is a row (or a repeat folded into one).
struct Answered {
    begun: Begun,
    /// The request's facts and the answer's (taken out of `begun`).
    facts: Facts,
    closed: Option<Box<str>>,
    lab: Option<Box<row::LabTag>>,
    class: errors::Class,
    idle: bool,
    request_bytes: u32,
    response_bytes: u32,
    took: u32,
}

/// Reads what the answer says: no shared table is needed for it, so it is done before the lock is taken.
fn answered(mut begun: Begun, seen: &Observed<'_>) -> Answered {
    let mut facts = std::mem::take(&mut begun.facts);
    let mut closed = None;
    match &seen.answer {
        Answer::Response(r) => summary::of_response(r, &mut facts),
        Answer::Nothing => {}
        Answer::Closed(why) => closed = Some(clip::clip(why, clip::TEXT)),
    }
    let class = if closed.is_some() {
        errors::Class::Bad
    } else {
        errors::class(facts.code)
    };
    let idle = closed.is_none()
        && class == errors::Class::Ok
        && match begun.api {
            ApiKey::Heartbeat | ApiKey::ConsumerGroupHeartbeat => true,
            ApiKey::Fetch => facts.records == 0 && facts.batches == 0,
            _ => false,
        };
    Answered {
        facts,
        closed,
        lab: seen.fault.map(|n| {
            Box::new(row::LabTag {
                rule: n.rule,
                name: clip::name(&n.name),
                applied: n.applied,
            })
        }),
        class,
        idle,
        request_bytes: u32::try_from(begun.request_bytes).unwrap_or(u32::MAX),
        response_bytes: u32::try_from(seen.response_bytes).unwrap_or(u32::MAX),
        took: u32::try_from(seen.took.as_micros()).unwrap_or(u32::MAX),
        begun,
    }
}

impl Answered {
    /// The row of a request that does not fold, answered at `answered_ms`.
    fn row(self, answered_ms: i64, fold: u64) -> Row {
        let b = self.begun;
        Row {
            id: 0,
            touched: 0,
            first_ms: answered_ms,
            last_ms: answered_ms,
            count: 1,
            conn: b.conn,
            who: b.who,
            api: b.api,
            version: b.version,
            correlation: b.correlation,
            code: self.facts.code,
            facts: Box::new(self.facts),
            class: self.class,
            closed: self.closed,
            request_bytes: self.request_bytes,
            response_bytes: self.response_bytes,
            tooks: vec![self.took],
            idle: self.idle,
            fold,
            lab: self.lab,
        }
    }
}

/// What an answer teaches: the names behind topic ids. A Metadata answer names every topic each time a client refreshes, so
/// an id already known with the same name costs nothing: the id is written out on the stack and looked up before anything
/// is copied.
fn learn(ids: &mut HashMap<Box<str>, Arc<str>>, r: &Response) {
    let mut put = |topic_id: kafka_wire::Uuid, name: &str| {
        let mut text = [0u8; 36];
        let id: &str = topic_id.hyphenated().encode_lower(&mut text);
        if let Some(known) = ids.get_mut(id) {
            if !clip::is(known, name) {
                *known = clip::name(name).into();
            }
        } else if ids.len() < MOST_IDS {
            ids.insert(id.into(), clip::name(name).into());
        }
    };
    match r {
        Response::Metadata(m) => {
            for t in &m.topics {
                if let Some(n) = &t.name {
                    put(t.topic_id, &n.0);
                }
            }
        }
        Response::CreateTopics(c) => {
            for t in &c.topics {
                put(t.topic_id, &t.name.0);
            }
        }
        _ => {}
    }
}

/// Puts the name of a topic in place of the id Fetch used, where it is known (shared with the table: no copy).
fn resolve(ids: &HashMap<Box<str>, Arc<str>>, f: &mut Facts) {
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

/// How a request that succeeded and that no Lab rule touched may fold.
fn fold_of(api: ApiKey, idle: bool, key: u64) -> Fold {
    if key == 0 {
        return Fold::Never;
    }
    match api {
        ApiKey::Heartbeat | ApiKey::ConsumerGroupHeartbeat => Fold::Poll,
        ApiKey::Fetch if idle => Fold::Poll,
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
fn fold_key(conn: u64, api: ApiKey, version: i16, f: &Facts) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (conn, api as i16, version, f.code, f.transactional).hash(&mut h);
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
#[cfg(test)]
mod tests_fold;
#[cfg(test)]
mod tests_topics;
