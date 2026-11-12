//! The answers that are not pages: the stream, the files, the health check.

use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::header::{ACCEPT_RANGES, CACHE_CONTROL, CONTENT_TYPE, ETAG};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::state::Shared;
use crate::stream::{self, WATCHABLE};

/// `GET /events?watch=a,b`.
pub async fn events(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let fields = crate::form::parse(query.as_deref().unwrap_or(""));
    let value = crate::form::get(&fields, "watch").unwrap_or("");
    let ids = match stream::parse_watch(value) {
        Ok(ids) => ids.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        Err(bad) => {
            let shown: String = bad.chars().take(40).collect();
            let body = format!(
                "{shown:?} is not a fragment this console can watch; the choices are {}",
                WATCHABLE.join(", ")
            );
            return (StatusCode::BAD_REQUEST, body).into_response();
        }
    };
    // One permit per open stream, held by the stream until it ends: the 65th is told to come back.
    let Ok(permit) = Arc::clone(&s.streams).try_acquire_owned() else {
        return crate::limits::busy("The console has as many event streams open as it allows.");
    };
    // What the page says each fragment's address is (its `data-frag`): with it the stream tells a change from a tick, per fragment.
    let watched: Vec<stream::Watched> = fields
        .iter()
        .filter(|(k, _)| k == "f")
        .filter_map(|(_, v)| {
            let path = s.base.strip(v.split('?').next()?)?;
            let id = path.strip_prefix("/fragments/")?;
            ids.iter().any(|i| i == id).then(|| stream::Watched {
                id: id.to_owned(),
                query: v.split_once('?').map(|(_, q)| q.to_owned()),
            })
        })
        .collect();
    let every = crate::form::get(&fields, "every").and_then(|v| v.parse::<u64>().ok());
    stream::response(
        s.clone(),
        s.ticker.watch(),
        ids,
        watched,
        every,
        s.stop.clone(),
        permit,
    )
    .into_response()
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
