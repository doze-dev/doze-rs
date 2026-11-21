//! `GET /api/v1/resources/<service>/<kind>/<name>/blockers`: what stands in the way of deleting a resource and what a delete
//! leaves ([`crate::resources::blockers`]). `DELETE …/<name>?clear=yes` ([`super::resources::delete`]) clears, then deletes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::json;
use super::resources::{find, refused};
use crate::resources::blockers;
use crate::state::Shared;

/// `GET /api/v1/resources/<service>/<kind>/<name>/blockers`.
pub async fn get(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    match blockers::read(&bus, (d, k), &name).await {
        Ok(read) => json(StatusCode::OK, &read),
        Err(e) => refused(&e),
    }
}
