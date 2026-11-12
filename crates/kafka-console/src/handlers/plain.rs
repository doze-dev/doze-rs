//! The answers that are not pages: the stream, the files, the health check.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
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
    stream::response(s.ticker.watch(), ids, s.stop.clone()).into_response()
}

/// A file of the manifest by its hashed name, cached for a year: the name changes when the file does.
pub async fn static_file(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    match s.assets.get(&name) {
        Some(f) => (
            [
                (CONTENT_TYPE, f.content_type),
                (CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            f.bytes.clone(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /healthz`.
pub async fn health() -> &'static str {
    "ok\n"
}
