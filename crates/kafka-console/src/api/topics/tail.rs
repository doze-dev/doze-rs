//! The Messages tail of the Topics API (slice R1a, CON-1104): server-sent events, server-paced, resumable.

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, StreamExt};
use serde::Serialize;

use super::{fields, no_topic, valid};
use crate::api::error;
use crate::form::get as field;
use crate::model::{MessageQuery, RecordRow};
use crate::state::Shared;

/// The most records a Messages tail looks at in one look.
pub const TAIL_WINDOW: usize = crate::stream::MESSAGES_WINDOW;

/// One event of the Messages tail: the rows a look found that the page did not have, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailRows {
    pub rows: Vec<RecordRow>,
}

/// What a tail follows: a topic's newest records, from a timestamp, with the keys of the last look.
struct Follow {
    topic: String,
    q: MessageQuery,
    since_ms: i64,
    last: Option<HashSet<(i32, i64)>>,
}

impl Follow {
    /// One look: the rows that are new since the last one (`None`: nothing new, nothing is sent).
    async fn look(&mut self, s: &Shared) -> Option<(i64, Vec<RecordRow>)> {
        let view = s
            .source
            .messages(&self.topic, self.q.clone(), vec![])
            .await?;
        let keys: HashSet<(i32, i64)> = view.rows.iter().map(|r| (r.partition, r.offset)).collect();
        let since = self.since_ms;
        let fresh: Vec<RecordRow> = view
            .rows
            .into_iter()
            .filter(|r| match &self.last {
                Some(prev) => !prev.contains(&(r.partition, r.offset)),
                None => r.timestamp_ms >= since,
            })
            .collect();
        self.last = Some(keys);
        let newest = fresh.iter().map(|r| r.timestamp_ms).max()?;
        self.since_ms = self.since_ms.max(newest);
        Some((self.since_ms, fresh))
    }
}

/// `GET /api/v1/tail/messages?topic=&pace=&since=&partition=&isolation=`: the Messages tail (CON-809), server-paced
/// (`pace` seconds between looks, 1 to 30) and coalesced: an event only when a look found new rows, its `id` the cursor
/// to resume from (`Last-Event-ID` on a reconnect, else `since`), so a reconnect brings what it missed and nothing twice.
pub(super) async fn tail_messages(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = fields(q.as_deref());
    let topic = crate::form::text(&f, "topic").to_owned();
    if !valid(&topic) {
        return no_topic(&topic);
    }
    let (mut query, _notes) = MessageQuery::parse(&f);
    if query.key.is_some() || query.offset.is_some() || query.time.is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A key, an offset or a time asks for a fixed read: only the newest records can be tailed.",
        );
    }
    query.limit = TAIL_WINDOW;
    if s.source.topic_head(&topic).await.is_none() {
        return no_topic(&topic);
    }
    let since = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .or_else(|| field(&f, "since"))
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let Ok(permit) = Arc::clone(&s.streams).try_acquire_owned() else {
        return crate::api::busy("The console has as many event streams open as it allows.");
    };
    let pace = crate::stream::pace_of(field(&f, "pace"));
    let follow = Follow {
        topic,
        q: query,
        since_ms: since,
        last: None,
    };
    let ready = stream::once(std::future::ready(Ok::<Event, Infallible>(
        Event::default().event("ready").data("ok"),
    )));
    let mut every = tokio::time::interval(pace);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let rest = stream::unfold(
        (s, follow, every, permit),
        |(s, mut follow, mut every, permit)| async move {
            loop {
                tokio::select! {
                    () = s.stop.cancelled() => return None,
                    _ = every.tick() => {}
                }
                if let Some((id, rows)) = follow.look(&s).await {
                    let data = serde_json::to_string(&TailRows { rows }).unwrap_or_default();
                    let ev = Event::default().event("rows").id(id.to_string()).data(data);
                    return Some((Ok(ev), (s, follow, every, permit)));
                }
            }
        },
    );
    Sse::new(ready.chain(rest))
        .keep_alive(
            KeepAlive::new()
                .interval(crate::stream::KEEP_ALIVE)
                .text("keep-alive"),
        )
        .into_response()
}
