//! `GET` and `PUT /api/v1/resources/<service>/<kind>/<name>/tags`: a resource's tags, read and changed through the service's own
//! calls ([`crate::resources::tags`]). A kind with no tags entry answers 404 `NO_TAGS`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::resources::{find, refused};
use super::{error, json};
use crate::model::TagsChange;
use crate::resources::{Descriptor, Kind, tags};
use crate::state::Shared;

fn tagged(k: &'static Kind) -> Result<&'static tags::Tags, Box<Response>> {
    k.tags.as_ref().ok_or_else(|| {
        Box::new(error(
            StatusCode::NOT_FOUND,
            "NO_TAGS",
            &format!("The console does not read the tags of a {}.", k.one),
        ))
    })
}

type Found = (
    crate::resources::bus::Bus,
    &'static Descriptor,
    &'static Kind,
);

fn kind_of(
    s: &Shared,
    service: &str,
    kind: &str,
) -> Result<(Found, &'static tags::Tags), Box<Response>> {
    let found = find(s, service, kind)?;
    let t = tagged(found.2)?;
    Ok((found, t))
}

/// `GET /api/v1/resources/<service>/<kind>/<name>/tags`.
pub async fn get(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
) -> Response {
    let ((bus, d, k), t) = match kind_of(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    match tags::read(&bus, (d, k), t, &name).await {
        Ok(view) => json(StatusCode::OK, &view),
        Err(e) => refused(&e),
    }
}

/// `PUT /api/v1/resources/<service>/<kind>/<name>/tags`.
pub async fn put(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let ((bus, d, k), t) = match kind_of(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let Ok(change) = serde_json::from_slice::<TagsChange>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A tags change is {\"set\": [{\"key\", \"value\"}], \"remove\": [\"key\"]}.",
        );
    };
    match tags::change(&bus, (d, k), t, &name, &change).await {
        Ok(view) => json(StatusCode::OK, &view),
        Err(e) => refused(&e),
    }
}
