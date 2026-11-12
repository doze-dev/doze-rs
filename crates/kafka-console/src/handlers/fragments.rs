//! The fragments a page fetches alone, and the form that asks for a new check.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use super::{etagged, html};
use crate::pages;
use crate::state::Shared;

/// A fragment by its id: the parts of Home that change while it is open, and the three renderings of the check.
pub async fn fragment(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let markup = match id.as_str() {
        pages::HOME_SENTENCE => pages::sentence_fragment(&s.source.home(None).await, &s.base),
        pages::HOME_CALLOUT => pages::callout_fragment(&s.source.home(None).await, &s.base),
        pages::HOME_INSTANCE => pages::instance_fragment(&s.source.home(None).await, &s.base),
        pages::PROBE_HOME => pages::probe_home(&s.source.probe(false).await, &s.base),
        pages::PROBE_CONNECT => pages::probe_connect(&s.source.probe(false).await, &s.base),
        pages::PROBE_ADVERTISED => pages::probe_advertised(&s.source.probe(false).await, &s.base),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    etagged(&headers, &markup)
}

/// Runs the advertised-listener check now. With a script the answer is the fragment to swap in; without one it is a
/// redirect back to the page, which shows the new result.
pub async fn check(State(s): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let fields = crate::form::parse(&String::from_utf8_lossy(&body));
    let (path, target) = match crate::form::get(&fields, "back").unwrap_or("") {
        "home" => ("/", pages::PROBE_HOME),
        "advertised" => ("/connect/advertised", pages::PROBE_ADVERTISED),
        _ => return (StatusCode::BAD_REQUEST, "back must be home or advertised").into_response(),
    };
    let probe = s.source.probe(true).await;
    if headers.contains_key("hx-request") {
        let markup = if target == pages::PROBE_HOME {
            pages::probe_home(&probe, &s.base)
        } else {
            pages::probe_advertised(&probe, &s.base)
        };
        return html(&markup);
    }
    Redirect::to(&s.base.url(path)).into_response()
}
