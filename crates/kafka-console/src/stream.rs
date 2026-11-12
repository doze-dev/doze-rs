//! `GET /events?tail=live|messages&...`: the console's only stream, for the two tails (CON-809, `docs/port/console/live-design.md`).
//!
//! A person has asked for it (and for how fast: `pace=1|3|10` seconds, never under one, never over thirty) (the `Live tail` control of the Live page or of a topic's Messages page); no other page opens a
//! stream. The server COALESCES: it looks once a second, and sends an event only when rows were added (or a folded row grew),
//! so the traffic rate of the cluster never becomes the update rate of the page. An event carries the rows as HTML, newest
//! first, and its `id` is the cursor the next look starts from: when the browser reconnects it sends that as `Last-Event-ID`,
//! so a reconnect brings the rows it missed and no row twice. The first event, `ready`, is sent at once and says the stream is
//! open. A comment keeps the connection alive every 15 s. Every stream ends when the server's cancellation token does, so a
//! graceful shutdown never waits on one.

use std::collections::HashSet;
use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use tokio::sync::OwnedSemaphorePermit;

use crate::model::{MessageQuery, WireQuery};
use crate::state::Shared;
use crate::ui::client::EVENT_ROWS;

/// The least time between two looks: the most events a second a tail can send, whatever it asks for.
pub const LEAST_PACE: u64 = 1;
/// The most a person can slow a tail to, in seconds (Realtime 1, Relaxed 3, Slow 10).
pub const MOST_PACE: u64 = 30;

/// The time between looks that a request for `pace` seconds is given: never faster than once a second, never slower than 30 s.
#[must_use]
pub fn pace_of(pace: Option<&str>) -> Duration {
    Duration::from_secs(
        pace.and_then(|p| p.parse::<u64>().ok())
            .unwrap_or(LEAST_PACE)
            .clamp(LEAST_PACE, MOST_PACE),
    )
}
/// The comment line sent every 15 s.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// The most records a Messages tail looks at in one look.
pub const MESSAGES_WINDOW: usize = 50;

/// What a stream follows.
#[derive(Debug)]
pub enum Feed {
    /// The Live tape under the filters in `q`; `cursor` is the recorder's version already sent.
    Live { q: WireQuery, cursor: u64 },
    /// A topic's newest records; `since_ms` is the timestamp of the newest the page holds, `last` the keys of the last look.
    Messages {
        topic: String,
        q: MessageQuery,
        since_ms: i64,
        last: Option<HashSet<(i32, i64)>>,
    },
}

/// The rows of one look, as HTML, with the cursor to ask from next time.
#[derive(Debug, PartialEq, Eq)]
pub struct Batch {
    pub id: String,
    pub html: String,
}

/// HTML in one SSE `data` field: a line break in a value is a character reference, so the field stays one line.
#[must_use]
pub fn one_line(html: &str) -> String {
    html.replace('\r', "&#13;").replace('\n', "&#10;")
}

impl Feed {
    /// The cursor to start from, as the event ids say it.
    pub fn resume(&mut self, id: &str) {
        match self {
            Self::Live { cursor, .. } => {
                if let Ok(n) = id.parse() {
                    *cursor = n;
                }
            }
            Self::Messages { since_ms, .. } => {
                if let Ok(n) = id.parse() {
                    *since_ms = n;
                }
            }
        }
    }

    /// One look. `None`: nothing was added, so nothing is sent.
    pub async fn look(&mut self, s: &Shared) -> Option<Batch> {
        match self {
            Self::Live { q, cursor } => {
                let (next, rows) = s.source.tail_since(q, *cursor).await;
                *cursor = next;
                if rows.is_empty() {
                    return None;
                }
                let html = crate::pages::live::tail_rows(&rows, &s.base, q).into_string();
                Some(Batch {
                    id: next.to_string(),
                    html: one_line(&html),
                })
            }
            Self::Messages {
                topic,
                q,
                since_ms,
                last,
            } => {
                let view = s.source.messages(topic, q.clone(), vec![]).await?;
                let keys: HashSet<(i32, i64)> =
                    view.rows.iter().map(|r| (r.partition, r.offset)).collect();
                let fresh: Vec<_> = view
                    .rows
                    .iter()
                    .filter(|r| match last {
                        Some(prev) => !prev.contains(&(r.partition, r.offset)),
                        None => r.timestamp_ms >= *since_ms,
                    })
                    .cloned()
                    .collect();
                *last = Some(keys);
                let newest = fresh.iter().map(|r| r.timestamp_ms).max()?;
                *since_ms = (*since_ms).max(newest);
                let html = crate::pages::topics::tail_rows(&fresh, topic, &q.to_query(), &s.base)
                    .into_string();
                Some(Batch {
                    id: since_ms.to_string(),
                    html: one_line(&html),
                })
            }
        }
    }
}

fn event(batch: &Batch) -> Event {
    Event::default()
        .event(EVENT_ROWS)
        .id(batch.id.clone())
        .data(&batch.html)
}

/// The events of one stream: `ready` at once, then one per look that found rows, until the console stops.
pub fn events(
    s: Shared,
    feed: Feed,
    pace: Duration,
    slot: OwnedSemaphorePermit,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let ready = stream::once(std::future::ready(Ok(Event::default()
        .event("ready")
        .data("ok"))));
    let mut every = tokio::time::interval(pace);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The permit lives as long as the stream does, and is given back when it is dropped.
    let rest = stream::unfold(
        (s, feed, every, slot),
        |(s, mut feed, mut every, slot)| async move {
            loop {
                tokio::select! {
                    () = s.stop.cancelled() => return None,
                    _ = every.tick() => {}
                }
                if let Some(batch) = feed.look(&s).await {
                    return Some((Ok(event(&batch)), (s, feed, every, slot)));
                }
            }
        },
    );
    ready.chain(rest)
}

/// The response of an open stream.
pub fn response(
    s: Shared,
    feed: Feed,
    pace: Duration,
    slot: OwnedSemaphorePermit,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(events(s, feed, pace, slot))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pace_is_what_was_asked_within_the_caps() {
        assert_eq!(pace_of(None), Duration::from_secs(1));
        assert_eq!(pace_of(Some("3")), Duration::from_secs(3));
        assert_eq!(
            pace_of(Some("0")),
            Duration::from_secs(1),
            "never faster than once a second"
        );
        assert_eq!(pace_of(Some("9999")), Duration::from_secs(30));
        assert_eq!(pace_of(Some("-4")), Duration::from_secs(1));
        assert_eq!(pace_of(Some("fast")), Duration::from_secs(1));
    }

    #[test]
    fn a_line_break_in_a_value_cannot_split_an_event() {
        assert_eq!(one_line("a\nb\r\nc"), "a&#10;b&#13;&#10;c");
        assert_eq!(one_line("<tr id=\"x\">"), "<tr id=\"x\">");
    }
}
