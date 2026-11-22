//! The reading of the Live tail page (`crate::tail`, CON-1581): which topics a tail follows now, where each partition ends,
//! and one bounded round of Fetches over the partitions that have something new.

use std::collections::HashMap;

use kafka_wire::api::Response;

use super::meta::{self, TopicMeta};
use super::read;
use super::walk::{Keep, Raw};
use crate::backend::Backend;
use crate::backend::requests::{self, Ask as OffsetAsk, EARLIEST, Isolation, LATEST};
use crate::model::Reading;
use crate::tail::{
    Follow, Hit, LOOK_BYTES, LOOK_FETCHES, Looked, MOST_PARTITIONS, MOST_TOPICS, Place, SKIP_PAST,
    SKIP_TO, Start, TailTopic,
};

/// What a tail keeps of each record: enough of the key and value to filter on and preview, the first bytes of each header.
const KEEP: Keep = Keep {
    key: 1024,
    value: 8 << 10,
    header: 256,
};
/// Kafka's OFFSET_OUT_OF_RANGE: the log start moved past where a partition stood.
const OUT_OF_RANGE: i16 = 1;

fn isolation(r: Reading) -> Isolation {
    match r {
        Reading::Uncommitted => Isolation::ReadUncommitted,
        Reading::Committed => Isolation::ReadCommitted,
    }
}

/// The offset each of `parts` answers for `at` (earliest or latest), by partition.
async fn offsets(
    b: &Backend,
    topic: &str,
    parts: &[i32],
    at: i64,
    reading: Reading,
) -> HashMap<i32, i64> {
    let asks: Vec<OffsetAsk> = parts
        .iter()
        .map(|&p| OffsetAsk {
            partition: p,
            timestamp: at,
        })
        .collect();
    let Ok(Response::ListOffsets(r)) = b
        .call(requests::list_offsets(topic, &asks, isolation(reading)))
        .await
    else {
        return HashMap::new();
    };
    r.topics
        .iter()
        .flat_map(|t| t.partitions.iter())
        .filter(|p| p.error_code == 0 && p.offset >= 0)
        .map(|p| (p.partition_index, p.offset))
        .collect()
}

/// The topics the ask names that exist, and those its patterns match, by name.
async fn wanted(b: &Backend, f: &Follow) -> Option<Vec<TopicMeta>> {
    let all = if f.ask.patterns.is_empty() {
        meta::named(b, &f.ask.names).await?
    } else {
        meta::all(b).await?
    };
    Some(
        all.into_iter()
            .filter(|m| f.ask.wants(&m.name, m.internal))
            .collect(),
    )
}

/// Where a partition first seen starts: the cursor a reconnect brought, the ask's start on the first look, its log start
/// after it (a topic or a partition that appeared while the tail ran: all of it is new).
fn first_next(f: &Follow, topic: &str, p: i32, start: Option<i64>, end: i64) -> i64 {
    if f.opened {
        return start.unwrap_or(0);
    }
    if let Some((_, _, o)) = f
        .resume
        .as_ref()
        .and_then(|r| r.iter().find(|(t, rp, _)| t == topic && *rp == p))
    {
        return *o;
    }
    match f.ask.start {
        Start::Now => end,
        Start::Last(n) => (end - n).max(start.unwrap_or(0)),
    }
}

/// The places of one topic, the old ones kept where they were (unless the topic was made again: a new id).
async fn places_of(b: &Backend, f: &Follow, m: &TopicMeta, parts: &[i32]) -> Vec<Place> {
    let ends = offsets(b, &m.name, parts, LATEST, f.ask.reading).await;
    let fresh: Vec<i32> = parts
        .iter()
        .copied()
        .filter(|p| {
            !f.places
                .iter()
                .any(|o| o.topic == m.name && o.partition == *p && o.id == m.id)
        })
        .collect();
    let needs_start = !fresh.is_empty() && (f.opened || matches!(f.ask.start, Start::Last(_)));
    let starts = if needs_start {
        offsets(b, &m.name, &fresh, EARLIEST, f.ask.reading).await
    } else {
        HashMap::new()
    };
    let mut out = Vec::with_capacity(parts.len());
    for &p in parts {
        let Some(&end) = ends.get(&p) else { continue };
        let old = f
            .places
            .iter()
            .find(|o| o.topic == m.name && o.partition == p && o.id == m.id);
        let next = match old {
            Some(o) if o.next <= end => o.next,
            // Past the end: the log was cut back (Empty topic); read from its start.
            Some(_) => offsets(b, &m.name, &[p], EARLIEST, f.ask.reading)
                .await
                .get(&p)
                .copied()
                .unwrap_or(end),
            None => first_next(f, &m.name, p, starts.get(&p).copied(), end).min(end),
        };
        out.push(Place {
            topic: m.name.clone(),
            id: m.id,
            partition: p,
            next,
            end,
        });
    }
    out
}

/// Settles which topics the tail follows now (within [`MOST_TOPICS`] and [`MOST_PARTITIONS`]) and where every partition
/// stands; the topics that came and went go in `out`.
async fn resolve(b: &Backend, f: &mut Follow, metas: &[TopicMeta], out: &mut Looked) {
    let mut places = Vec::new();
    let mut topics = Vec::new();
    let mut left_out = 0;
    for m in metas {
        let parts: Vec<i32> = m
            .partitions
            .iter()
            .copied()
            .filter(|p| f.ask.partitions.is_empty() || f.ask.partitions.contains(p))
            .collect();
        if topics.len() >= MOST_TOPICS || places.len() + parts.len() > MOST_PARTITIONS {
            left_out += 1;
            continue;
        }
        let known = f.topics.iter().any(|t| t.name == m.name);
        let t = TailTopic {
            name: m.name.clone(),
            partitions: i32::try_from(m.partitions.len()).unwrap_or(i32::MAX),
        };
        if f.opened && !known {
            out.joined.push(t.clone());
        }
        places.extend(places_of(b, f, m, &parts).await);
        topics.push(t);
    }
    out.left = f
        .topics
        .iter()
        .filter(|t| !topics.iter().any(|n: &TailTopic| n.name == t.name))
        .map(|t| t.name.clone())
        .collect();
    f.places = places;
    f.topics = topics;
    f.left_out = left_out;
    f.resume = None;
}

fn hit(topic: &str, r: Raw) -> Hit {
    Hit {
        topic: topic.to_owned(),
        partition: r.partition,
        offset: r.offset,
        timestamp_ms: r.timestamp,
        key: r.key,
        key_len: r.key_len,
        value: r.value,
        value_len: r.value_len,
        headers: r.headers,
        standing: r.standing,
    }
}

/// Moves each partition more than [`SKIP_PAST`] records behind to its newest [`SKIP_TO`]; the records passed over.
fn skip_far_behind(places: &mut [Place]) -> u64 {
    let mut skipped = 0;
    for p in places {
        let behind = p.end - p.next;
        if behind > SKIP_PAST {
            skipped += u64::try_from(behind - SKIP_TO).unwrap_or(0);
            p.next = p.end - SKIP_TO;
        }
    }
    skipped
}

/// Reads one Fetch of place `i`; whether it moved.
async fn fetch_one(b: &Backend, f: &mut Follow, i: usize, out: &mut Looked) -> (bool, usize) {
    let reading = f.ask.reading;
    let Some(p) = f.places.get(i) else {
        return (false, 0);
    };
    let meta = TopicMeta {
        name: p.topic.clone(),
        id: p.id,
        partitions: Vec::new(),
        internal: false,
    };
    let (partition, from) = (p.partition, p.next);
    let c = read::once(b, &meta, (partition, from), reading, (KEEP, usize::MAX)).await;
    if c.error == Some(OUT_OF_RANGE) {
        let start = offsets(b, &meta.name, &[partition], EARLIEST, reading).await;
        if let (Some(p), Some(&s)) = (f.places.get_mut(i), start.get(&partition)) {
            let moved = s > p.next;
            p.next = s.max(p.next);
            return (moved, 0);
        }
        return (false, 0);
    }
    out.count(&meta.name, c.rows.len() as u64);
    out.hits
        .extend(c.rows.into_iter().map(|r| hit(&meta.name, r)));
    let moved = match (f.places.get_mut(i), c.next) {
        (Some(p), Some(n)) if n > p.next => {
            p.next = n;
            p.end = p.end.max(n);
            true
        }
        _ => false,
    };
    (moved, c.bytes)
}

/// One bounded round: at most [`LOOK_FETCHES`] Fetches and [`LOOK_BYTES`] of records, a Fetch per partition with something
/// new in turn, starting where the last look's turn left off.
async fn read_round(b: &Backend, f: &mut Follow, out: &mut Looked) {
    out.skipped += skip_far_behind(&mut f.places);
    let n = f.places.len();
    if n == 0 {
        return;
    }
    let (mut fetches, mut bytes) = (0_usize, 0_usize);
    let mut moved = true;
    while moved {
        moved = false;
        for k in 0..n {
            if fetches >= LOOK_FETCHES || bytes >= LOOK_BYTES {
                f.turn = (f.turn + k) % n;
                return;
            }
            let i = (f.turn + k) % n;
            if f.places.get(i).is_none_or(|p| p.next >= p.end) {
                continue;
            }
            let (m, got) = fetch_one(b, f, i, out).await;
            fetches += 1;
            bytes += got;
            moved |= m;
        }
    }
    f.turn = (f.turn + 1) % n;
}

/// One look of a tail: the topics settled, then (after the first) one bounded round of reads. A Metadata that fails changes
/// nothing.
pub async fn look(b: &Backend, f: &mut Follow) -> Looked {
    let mut out = Looked::default();
    let Some(metas) = wanted(b, f).await else {
        return out;
    };
    resolve(b, f, &metas, &mut out).await;
    // The first look only settles where every partition starts (what `ready` says); the reads begin with the next.
    if !f.opened {
        f.opened = true;
        return out;
    }
    read_round(b, f, &mut out).await;
    out
}
