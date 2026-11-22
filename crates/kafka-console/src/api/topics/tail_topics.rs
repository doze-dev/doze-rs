//! `GET /api/v1/tail/topics`: the Live tail page's stream (CON-1580 to CON-1585), server-sent events, server-paced, filtered
//! here, resumable by its cursor.
//!
//! `ready` says what is followed and from where; then a `batch` for each look that read something or found the topics changed
//! (none while nothing happens: the console does not talk to be talking), its `id` the cursor of every partition, so a
//! reconnect (`Last-Event-ID`) goes on where the last batch left off: nothing twice, nothing missed within the bounds of
//! `crate::tail`. Nothing runs for a tail whose client went away: the stream is only read while it is being sent.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, StreamExt};

use super::{fields, valid};
use crate::api::error;
use crate::form::get as field;
use crate::gather::decode::{clean, shown};
use crate::model::{HeaderRow, Shown};
use crate::schemas::Codecs;
use crate::state::Shared;
use crate::tail::{
    Ask, EVENT_HEADERS, EVENT_ROWS, Follow, Hit, Looked, MOST_PARTITIONS, MOST_TOPICS, Start,
    TailCount, TailRecord, TopicsTailBatch, TopicsTailReady,
};
use crate::tail_filter::Candidate;

/// How much of a value's text an event carries (the record Sheet reads the whole of it).
const SENT_TEXT: usize = 512;

/// `text` cut to at most `cap` bytes, on a character.
fn cut(text: &mut String, cap: usize) -> bool {
    if text.len() <= cap {
        return false;
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    true
}

/// What a row carries of a key or a value: its text cut to [`SENT_TEXT`], its schema's laid-out JSON left out.
fn slim(mut s: Shown) -> Shown {
    if cut(&mut s.text, SENT_TEXT) {
        s.clipped = true;
    }
    if let Some(k) = s.schema.as_mut() {
        k.json = None;
    }
    s
}

/// The text the value filters read: its schema's JSON when it has one, else its text.
fn value_text(s: &Shown) -> Option<&str> {
    if s.kind == crate::model::Kind::Null {
        return None;
    }
    Some(
        s.schema
            .as_ref()
            .and_then(|k| k.json.as_deref())
            .unwrap_or(&s.text),
    )
}

/// One record read: kept, as a row, if the filters hold.
fn row_of(s: &Shared, f: &Follow, h: Hit, codecs: &mut Codecs) -> Option<TailRecord> {
    let headers: Vec<(String, String)> = h
        .headers
        .iter()
        .map(|(k, v, _)| {
            let value = v
                .as_deref()
                .map(String::from_utf8_lossy)
                .unwrap_or_default();
            (String::from_utf8_lossy(k).into_owned(), value.into_owned())
        })
        .collect();
    let key = shown(h.key.as_deref(), h.key_len);
    let mut value = shown(h.value.as_deref(), h.value_len);
    let schemas = s.source.schemas();
    schemas.decorate(&mut value, &h.topic, false, codecs);
    let keep = f.ask.filter.matches(&Candidate {
        key: (key.kind != crate::model::Kind::Null).then_some(key.text.as_str()),
        value: value_text(&value),
        headers: &headers,
    });
    if !keep {
        return None;
    }
    let mut key = key;
    schemas.decorate(&mut key, &h.topic, true, codecs);
    let total = u32::try_from(h.headers.len()).unwrap_or(u32::MAX);
    Some(TailRecord {
        headers: h
            .headers
            .iter()
            .take(EVENT_HEADERS)
            .map(|(k, v, n)| HeaderRow {
                key: clean(&String::from_utf8_lossy(k)),
                value: slim(shown(v.as_deref(), *n)),
            })
            .collect(),
        headers_total: total,
        topic: h.topic,
        partition: h.partition,
        offset: h.offset,
        timestamp_ms: h.timestamp_ms,
        key: slim(key),
        value: slim(value),
        standing: h.standing,
    })
}

/// The event for a look; `None` when it read nothing and nothing changed.
fn batch_of(s: &Shared, f: &Follow, mut looked: Looked) -> Option<TopicsTailBatch> {
    let scanned: u64 = looked.scanned.iter().map(|(_, n)| n).sum();
    if scanned == 0 && looked.skipped == 0 && looked.joined.is_empty() && looked.left.is_empty() {
        return None;
    }
    looked.hits.sort_by(|a, b| {
        (b.timestamp_ms, &a.topic, a.partition, b.offset).cmp(&(
            a.timestamp_ms,
            &b.topic,
            b.partition,
            a.offset,
        ))
    });
    let mut counts: Vec<TailCount> = looked
        .scanned
        .iter()
        .map(|(t, n)| TailCount {
            topic: t.clone(),
            scanned: *n,
            matched: 0,
        })
        .collect();
    let mut codecs = Codecs::default();
    let mut rows = Vec::new();
    let mut matched = 0_u64;
    let all = f.ask.filter.is_empty();
    for h in looked.hits {
        // With no filter every record is kept: only the newest are made into rows.
        let topic = h.topic.clone();
        let kept = if all && rows.len() >= EVENT_ROWS {
            true
        } else {
            match row_of(s, f, h, &mut codecs) {
                Some(r) => {
                    if rows.len() < EVENT_ROWS {
                        rows.push(r);
                    }
                    true
                }
                None => false,
            }
        };
        if kept {
            matched += 1;
            if let Some(c) = counts.iter_mut().find(|c| c.topic == topic) {
                c.matched += 1;
            }
        }
    }
    Some(TopicsTailBatch {
        rows,
        scanned,
        matched,
        counts,
        skipped: looked.skipped,
        joined: looked.joined,
        left: looked.left,
        cursor: f.cursor(),
        at_ms: crate::source::now_ms(),
    })
}

fn notes(f: &Follow) -> Vec<String> {
    let mut out = Vec::new();
    if f.topics.is_empty() {
        out.push(if f.ask.patterns.is_empty() {
            "None of these topics exists yet: each joins the tail when it is created.".to_owned()
        } else {
            "No topic matches yet: a topic created with a matching name joins the tail.".to_owned()
        });
    }
    if f.left_out > 0 {
        out.push(format!(
            "{} more matching {} not followed: a tail follows at most {MOST_TOPICS} topics and {MOST_PARTITIONS} partitions.",
            f.left_out,
            if f.left_out == 1 { "topic is" } else { "topics are" }
        ));
    }
    out
}

fn event(s: &Shared, f: &Follow, looked: Looked) -> Option<Event> {
    let b = batch_of(s, f, looked)?;
    let data = serde_json::to_string(&b).unwrap_or_default();
    Some(Event::default().event("batch").id(b.cursor).data(data))
}

/// `GET /api/v1/tail/topics?topic=&pattern=&from=now|last&last=&partition=&key=&value=&header=&header_value=&field=&equals=
/// &isolation=&pace=`: see the module.
pub(super) async fn tail_topics(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = fields(q.as_deref());
    let ask = match Ask::parse(&f, valid) {
        Ok(a) => a,
        Err(words) => return error(StatusCode::BAD_REQUEST, "BAD_REQUEST", &words),
    };
    let resume = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .or_else(|| field(&f, "cursor"))
        .map(str::to_owned);
    let Ok(permit) = Arc::clone(&s.streams).try_acquire_owned() else {
        return crate::api::busy("The console has as many event streams open as it allows.");
    };
    let pace = crate::stream::pace_of(field(&f, "pace"));
    let mut follow = Follow::new(ask, resume.as_deref());
    // The first look settles the topics and where each partition starts; it reads nothing.
    s.source.tail_look(&mut follow).await;
    let ready = TopicsTailReady {
        topics: follow.topics.clone(),
        cursor: follow.cursor(),
        notes: notes(&follow),
        pace_secs: pace.as_secs(),
        most_topics: MOST_TOPICS,
        most_partitions: MOST_PARTITIONS,
    };
    // The last N of each partition, or what a reconnect missed, is read at once after `ready`; from now waits a pace.
    let at_once = follow.ask.start != Start::Now || resume.is_some();
    let head = stream::once(std::future::ready(Ok::<Event, Infallible>(
        Event::default()
            .event("ready")
            .data(serde_json::to_string(&ready).unwrap_or_default()),
    )));
    let mut every = tokio::time::interval_at(tokio::time::Instant::now() + pace, pace);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let rest = stream::unfold(
        (s, follow, every, permit, at_once),
        |(s, mut follow, mut every, permit, mut at_once)| async move {
            loop {
                if at_once {
                    at_once = false;
                } else {
                    tokio::select! {
                        () = s.stop.cancelled() => return None,
                        _ = every.tick() => {}
                    }
                }
                let looked = s.source.tail_look(&mut follow).await;
                if let Some(ev) = event(&s, &follow, looked) {
                    return Some((Ok(ev), (s, follow, every, permit, at_once)));
                }
            }
        },
    );
    Sse::new(head.chain(rest))
        .keep_alive(
            KeepAlive::new()
                .interval(crate::stream::KEEP_ALIVE)
                .text("keep-alive"),
        )
        .into_response()
}
