//! `GET /api/v1/resources/lambda/functions/<name>/code`: the function's package, the `.zip` it was deployed as, for the Code tab
//! to read in the browser. GetFunction names where it is (`Code.Location`); Lambda answers a download of it as it answers the
//! gateway's, here without the network. The console reads the archive itself and sends a change back with UpdateFunctionCode.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::error;
use super::resources::{find, refused};
use crate::resources::path;
use crate::state::Shared;

/// Where Lambda serves packages from, after the endpoint (what `Code.Location` names).
const PACKAGES: &str = "/_aws/lambda/code/";

/// `GET …/lambda/functions/<name>/code`.
pub async fn code(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
) -> Response {
    if (service.as_str(), kind.as_str()) != ("lambda", "functions") {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Only a Lambda function has a package to read.",
        );
    }
    let (bus, d, _) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let got = match bus
        .call(d.target, "GetFunction", &json!({ "FunctionName": name }))
        .await
    {
        Ok(v) => v,
        Err(e) => return refused(&e),
    };
    // An image function has no package, and a function that is not there is refused above.
    let Some(id) = path::text(&got, "Code.Location")
        .and_then(|l| l.split_once(PACKAGES).map(|(_, rest)| rest.to_owned()))
    else {
        return error(
            StatusCode::NOT_FOUND,
            "NO_PACKAGE",
            &format!("{name} has no .zip package to read: its code is a container image."),
        );
    };
    let Some(reply) = bus.web_get(d.target, &format!("{PACKAGES}{id}")).await else {
        return error(
            StatusCode::BAD_GATEWAY,
            "NO_PACKAGE",
            "Lambda is not serving packages.",
        );
    };
    if reply.status != 200 {
        return error(
            StatusCode::NOT_FOUND,
            "NO_PACKAGE",
            &format!("Lambda has no package for {name} on disk."),
        );
    }
    let mut res = (StatusCode::OK, Body::from(reply.body)).into_response();
    res.headers_mut().insert(
        CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/zip"),
    );
    res.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    res
}
