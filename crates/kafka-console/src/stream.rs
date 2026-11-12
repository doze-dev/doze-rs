//! `GET /events?watch=a,b`: one server-sent stream per page on screen, carrying only "these fragments changed".
//!
//! The first event names every watched fragment, so a page that has just loaded (or reconnected) reconciles itself; each
//! later event is a tick of the shared signal ([`crate::tick`]) naming them again, and the page fetches them with an
//! ETag so an unchanged one is a 304. A comment keeps the connection alive every 15 s. Every stream ends when the
//! server's cancellation token does, so a graceful shutdown never waits on one.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

use crate::tick::Viewer;
use crate::ui::client::EVENT_TICK;

/// The fragments a stream may be asked to watch: a closed vocabulary.
pub const WATCHABLE: &[&str] = &[
    "home-live",
    "topics-sentence",
    "topics-table",
    "topic-sentence",
    "topic-messages",
];

/// What the Home page watches.
pub const HOME_IDS: &[&str] = &["home-live"];
/// What the topics list watches.
pub const TOPICS_IDS: &[&str] = &["topics-sentence", "topics-table"];
/// What a topic's Messages page watches.
pub const TOPIC_IDS: &[&str] = &["topic-sentence", "topic-messages"];

/// The comment line sent every 15 s.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The ids of a `watch` value, or the first one that is not in the vocabulary. An empty value watches nothing.
///
/// # Errors
/// The first id that is not watchable.
pub fn parse_watch(value: &str) -> Result<Vec<&str>, &str> {
    let mut out = Vec::new();
    for id in value.split(',').filter(|s| !s.is_empty()) {
        if !WATCHABLE.contains(&id)
            && !crate::stream_groups::WATCHABLE.contains(&id)
            && !crate::live_ids::WATCHABLE.contains(&id)
            && !crate::story_ids::WATCHABLE.contains(&id)
        {
            return Err(id);
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

fn tick(ids: &str) -> Event {
    Event::default().event(EVENT_TICK).data(ids)
}

/// A fragment a stream may compare: its id and the query its `data-frag` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub id: String,
    pub query: Option<String>,
}

/// The least time between two events of a stream that knows its fragments: a tape (Live) moves as fast as a person can follow at
/// once a second, Home at most every 10 s, everything else every 5 s. A hint (`every`, in seconds, capped at a minute) may only slow it.
#[must_use]
pub fn interval(ids: &[String], every: Option<u64>) -> Duration {
    let base = if ids
        .iter()
        .any(|i| crate::live_ids::WATCHABLE.contains(&i.as_str()))
    {
        1
    } else if ids.iter().all(|i| i == "home-live") {
        10
    } else {
        5
    };
    Duration::from_secs(base.max(every.unwrap_or(0).min(60)))
}

/// What a fragment says, without what only time says (its relative times): equal prints are an unchanged fragment.
async fn print(s: &crate::state::Shared, w: &Watched) -> Option<String> {
    let m = crate::handlers::fragment_markup(s, &w.id, &w.query).await?;
    Some(crate::assets::fingerprint(
        crate::live_updates::without_relative_times(&m.0).as_bytes(),
    ))
}

struct Run {
    s: crate::state::Shared,
    viewer: Viewer,
    stop: CancellationToken,
    /// Held for as long as the stream lives: the permit is given back when it is dropped.
    _slot: OwnedSemaphorePermit,
    data: String,
    watched: Vec<Watched>,
    prints: std::collections::HashMap<String, String>,
    every: Duration,
    last: tokio::time::Instant,
}

impl Run {
    /// Waits for the next tick that is worth sending: a bump of the signal, then (at most one answer per `every`, the last change
    /// never lost: the wait is the trailing edge) the fragments that really changed since this stream last named them.
    async fn next(&mut self) -> Option<String> {
        loop {
            let heard = tokio::select! {
                () = self.stop.cancelled() => false,
                heard = self.viewer.changed() => heard,
            };
            if !heard {
                return None;
            }
            if self.watched.is_empty() {
                return Some(self.data.clone());
            }
            let due = self.last + self.every;
            tokio::select! {
                () = self.stop.cancelled() => return None,
                () = tokio::time::sleep_until(due) => {}
            }
            self.last = tokio::time::Instant::now();
            let mut changed = Vec::new();
            for w in self.watched.clone() {
                if let Some(now) = print(&self.s, &w).await {
                    if self.prints.get(&w.id) != Some(&now) {
                        self.prints.insert(w.id.clone(), now);
                        changed.push(w.id);
                    }
                }
            }
            if !changed.is_empty() {
                return Some(changed.join(","));
            }
        }
    }
}

/// Where a stream is: about to send its first event, or waiting for the next one worth sending.
enum Phase {
    First(Box<Run>),
    Next(Box<Run>),
}

/// The events of one stream: the first at once (it names everything, and the stream remembers what each fragment says), then one per
/// tick worth sending, until `stop` is cancelled or the signal is gone.
pub fn events(
    s: crate::state::Shared,
    viewer: Viewer,
    ids: Vec<String>,
    watched: Vec<Watched>,
    every: Option<u64>,
    stop: CancellationToken,
    slot: OwnedSemaphorePermit,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let every = if watched.is_empty() {
        Duration::ZERO
    } else {
        interval(&ids, every)
    };
    let run = Run {
        s,
        viewer,
        stop,
        _slot: slot,
        data: ids.join(","),
        watched,
        prints: std::collections::HashMap::new(),
        every,
        last: tokio::time::Instant::now(),
    };
    stream::unfold(Phase::First(Box::new(run)), |phase| async move {
        match phase {
            Phase::First(mut run) => {
                for w in run.watched.clone() {
                    if let Some(p) = print(&run.s, &w).await {
                        run.prints.insert(w.id.clone(), p);
                    }
                }
                run.last = tokio::time::Instant::now();
                let event = tick(&run.data);
                Some((Ok(event), Phase::Next(run)))
            }
            Phase::Next(mut run) => {
                let ids = run.next().await?;
                Some((Ok(tick(&ids)), Phase::Next(run)))
            }
        }
    })
}

/// The response of an open stream.
pub fn response(
    s: crate::state::Shared,
    viewer: Viewer,
    ids: Vec<String>,
    watched: Vec<Watched>,
    every: Option<u64>,
    stop: CancellationToken,
    slot: OwnedSemaphorePermit,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(events(s, viewer, ids, watched, every, stop, slot))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_answered_no_oftener_than_a_person_can_follow() {
        let ids = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            interval(&ids(&["live-wire"]), None),
            Duration::from_secs(1),
            "a tape: once a second"
        );
        assert_eq!(
            interval(&ids(&["home-live"]), None),
            Duration::from_secs(10),
            "Home: every 10 s"
        );
        assert_eq!(
            interval(&ids(&["groups-live"]), None),
            Duration::from_secs(5),
            "state: every 5 s"
        );
        assert_eq!(
            interval(&ids(&["live-wire"]), Some(30)),
            Duration::from_secs(30),
            "a hint slows it"
        );
        assert_eq!(
            interval(&ids(&["live-wire"]), Some(1)),
            Duration::from_secs(1),
            "and never speeds it"
        );
        assert_eq!(
            interval(&ids(&["live-wire"]), Some(86_400)),
            Duration::from_secs(60),
            "and is capped at a minute"
        );
    }

    #[test]
    fn only_the_closed_vocabulary_may_be_watched() {
        assert_eq!(parse_watch(""), Ok(vec![]));
        assert_eq!(parse_watch("home-live"), Ok(vec!["home-live"]));
        assert_eq!(parse_watch("home-live,home-live"), Ok(vec!["home-live"]));
        assert_eq!(parse_watch("home-live,evil"), Err("evil"));
        assert_eq!(parse_watch("home-instance"), Err("home-instance"));
        assert_eq!(parse_watch("../x"), Err("../x"));
        assert_eq!(parse_watch("a\nb"), Err("a\nb"));
    }
}
