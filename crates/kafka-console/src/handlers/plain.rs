//! The answers that are not pages: the stream, the files, the health check.

use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::header::{ACCEPT_RANGES, CACHE_CONTROL, CONTENT_TYPE, ETAG};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::model::{MessageQuery, WireQuery};
use crate::state::Shared;
use crate::stream::{self, Feed};

/// `GET /events?tail=live&...` or `?tail=messages&topic=...`: the stream of one tail (CON-809). `since` (or the browser's
/// `Last-Event-ID`) is the cursor the page already holds.
pub async fn events(
    State(s): State<Shared>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let fields = crate::form::parse(query.as_deref().unwrap_or(""));
    let cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| crate::form::get(&fields, "since").map(str::to_owned));
    let mut feed = match crate::form::get(&fields, "tail") {
        Some("live") => {
            let q = WireQuery {
                before: None,
                ..WireQuery::parse(&fields)
            };
            Feed::Live { q, cursor: 0 }
        }
        Some("messages") => {
            let topic = crate::form::text(&fields, "topic").to_owned();
            let (q, _notes) = MessageQuery::parse(&fields);
            Feed::Messages {
                topic,
                q,
                since_ms: 0,
                last: None,
            }
        }
        _ => {
            return (StatusCode::BAD_REQUEST, "tail must be live or messages").into_response();
        }
    };
    if let Some(c) = &cursor {
        feed.resume(c);
    }
    // One permit per open stream, held by the stream until it ends: the 65th is told to come back.
    let Ok(permit) = Arc::clone(&s.streams).try_acquire_owned() else {
        return crate::limits::busy("The console has as many event streams open as it allows.");
    };
    let pace = stream::pace_of(crate::form::get(&fields, "pace"));
    stream::response(s, feed, pace, permit).into_response()
}

/// A file of the manifest by its hashed name, cached for a year: the name changes when the file does.
/// The hashed name is the ETag (it changes when the file does), so a browser that asks again gets a 304. A `Range` is
/// ignored: the whole file is the answer (it is a few KB to 40 KB) and `Accept-Ranges: none` says so.
pub async fn static_file(
    State(s): State<Shared>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(f) = s.assets.get(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tag = format!("\"{name}\"");
    let mut res = if super::has_tag(&headers, &tag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(CONTENT_TYPE, f.content_type)], f.bytes.clone()).into_response()
    };
    let h = res.headers_mut();
    h.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    h.insert(ACCEPT_RANGES, HeaderValue::from_static("none"));
    if let Ok(v) = HeaderValue::from_str(&tag) {
        h.insert(ETAG, v);
    }
    res
}

/// `GET /healthz`.
pub async fn health() -> &'static str {
    "ok\n"
}
