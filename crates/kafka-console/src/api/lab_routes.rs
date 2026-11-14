//! The Lab's addresses, and the two that teach and stop: `GET /api/v1/lab/guide` (how the Lab works, each rehearsal taught,
//! recipes naming the broker's own topics, apps and groups) and `POST /api/v1/lab/stop-all` (every rule in force ends now).

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use serde::Serialize;

use super::json;
use super::lab::{self, off, parts};
use crate::gather::lab as g;
use crate::state::Shared;

/// What Stop all came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabStopped {
    /// How many rules were in force and ended.
    pub stopped: usize,
}

/// `GET /api/v1/lab/guide`. A broker without the Lab answers the words with no recipe (its page says the Lab is off).
pub(super) async fn guide(State(s): State<Shared>) -> Response {
    let live = parts(&s).map(|p| (p.backend, p.recorder));
    json(StatusCode::OK, &g::guide(live).await)
}

/// `POST /api/v1/lab/stop-all`: ends every rule in force; they stay in Ended for an hour.
pub(super) async fn stop_all(State(s): State<Shared>) -> Response {
    let Some(p) = parts(&s) else { return off() };
    json(
        StatusCode::OK,
        &LabStopped {
            stopped: g::stop_all(p.lab),
        },
    )
}

/// The Lab's routes under `web`.
pub fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(&web.url("/api/v1/lab"), get(lab::home))
        .route(&web.url("/api/v1/lab/guide"), get(guide))
        .route(&web.url("/api/v1/lab/stop-all"), post(stop_all))
        .route(&web.url("/api/v1/lab/form"), get(lab::form))
        .route(&web.url("/api/v1/lab/rules"), post(lab::make))
        .route(&web.url("/api/v1/lab/rules/{id}"), get(lab::rule))
        .route(
            &web.url("/api/v1/lab/rules/{id}/enabled"),
            post(lab::switch),
        )
        .route(&web.url("/api/v1/lab/rules/{id}/remove"), post(lab::remove))
        .route(&web.url("/api/v1/lab/history/clear"), post(lab::clear))
        .route(
            &web.url("/api/v1/lab/once/{kind}"),
            get(lab::once_form).post(lab::once),
        )
        .route(
            &web.url("/api/v1/lab/compact"),
            get(lab::compact_form).post(lab::compact),
        )
}
