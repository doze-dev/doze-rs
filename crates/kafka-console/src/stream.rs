//! `GET /events?watch=a,b`: one server-sent stream per page on screen, carrying only "these fragments changed".
//!
//! The first event names every watched fragment, so a page that has just loaded (or reconnected) reconciles itself; each
//! later event is a tick of the shared signal ([`crate::tick`]) naming them again, and the page fetches them with an
//! ETag so an unchanged one is a 304. A comment keeps the connection alive every 15 s. Every stream ends when the
//! server's cancellation token does, so a graceful shutdown never waits on one.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::tick::Viewer;
use crate::ui::client::EVENT_TICK;

/// The fragments a stream may be asked to watch: a closed vocabulary.
pub const WATCHABLE: &[&str] = &["home-sentence", "home-callout", "home-instance"];

/// The comment line sent every 15 s.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The ids of a `watch` value, or the first one that is not in the vocabulary. An empty value watches nothing.
///
/// # Errors
/// The first id that is not watchable.
pub fn parse_watch(value: &str) -> Result<Vec<&str>, &str> {
    let mut out = Vec::new();
    for id in value.split(',').filter(|s| !s.is_empty()) {
        if !WATCHABLE.contains(&id) {
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

/// The events of one stream: the first at once, then one per tick, until `stop` is cancelled or the signal is gone.
pub fn events(
    viewer: Viewer,
    ids: Vec<String>,
    stop: CancellationToken,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let data = ids.join(",");
    let first = stream::once(std::future::ready(Ok(tick(&data))));
    let rest = stream::unfold(
        (viewer, stop, data),
        |(mut viewer, stop, data)| async move {
            let heard = tokio::select! {
                () = stop.cancelled() => false,
                heard = viewer.changed() => heard,
            };
            heard.then(|| (Ok(tick(&data)), (viewer, stop, data)))
        },
    );
    first.chain(rest)
}

/// The response of an open stream.
pub fn response(
    viewer: Viewer,
    ids: Vec<String>,
    stop: CancellationToken,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(events(viewer, ids, stop))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_closed_vocabulary_may_be_watched() {
        assert_eq!(parse_watch(""), Ok(vec![]));
        assert_eq!(parse_watch("home-instance"), Ok(vec!["home-instance"]));
        assert_eq!(
            parse_watch("home-sentence,home-instance,home-sentence"),
            Ok(vec!["home-sentence", "home-instance"])
        );
        assert_eq!(parse_watch("home-instance,evil"), Err("evil"));
        assert_eq!(parse_watch("../x"), Err("../x"));
        assert_eq!(parse_watch("a\nb"), Err("a\nb"));
    }
}
