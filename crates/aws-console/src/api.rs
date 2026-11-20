//! The JSON API of the console: the view-models of [`crate::model`], serialised, under `/api/v1/`.
//!
//! Every answer is JSON. A refusal is `{ "code": "...", "message": "..." }`. Every request must carry `X-Console: 1` (the
//! console's fetch client sets it): a page on another site cannot send a custom header without a CORS preflight, which this
//! server never grants. A request that is not a plain read must also pass the same-origin proof of [`crate::origin`]: the Resources
//! space's creates, deletes and reveals ([`resources`]) are behind it.

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN, RETRY_AFTER};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;

use crate::model::ApiError;
use crate::origin;
use crate::state::Shared;

pub mod about;
pub mod calls;
pub mod copy;
pub mod deep;
pub mod flows;
pub mod resources;
#[cfg(test)]
mod types;

/// The header the console's fetch client sends on every API request.
pub const PROOF_HEADER: &str = "x-console";

const FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

/// The API's routes, each behind [`guard`].
pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/v1/about", get(about::about))
        .route("/api/v1/services", get(about::services))
        .route("/api/v1/calls", get(calls::calls))
        .route("/api/v1/tail/live", get(calls::tail))
        .route("/api/v1/calls/{seq}/copy", get(copy::copy))
        .route("/api/v1/flows", get(flows::flows))
        .route("/api/v1/resources", get(resources::index))
        .route(
            "/api/v1/resources/{service}/{kind}",
            get(resources::list).post(resources::create),
        )
        .route(
            "/api/v1/resources/{service}/{kind}/{name}",
            get(resources::detail).delete(resources::delete),
        )
        .route(
            "/api/v1/resources/{service}/{kind}/{name}/reveal",
            post(resources::reveal),
        )
        .route(
            "/api/v1/resources/{service}/{kind}/{name}/op/{operation}",
            post(deep::op),
        )
        .route(
            "/api/v1/resources/{service}/{kind}/{name}/object",
            get(deep::download)
                .post(deep::upload)
                .layer(DefaultBodyLimit::max(deep::UPLOAD_MAX)),
        )
        .route(
            "/api/v1/resources/{service}/{kind}/{name}/tail",
            get(deep::tail),
        )
        .route_layer(from_fn(guard))
}

/// A value as a JSON answer. (axum's own `Json` needs its `json` feature, which brings a package the build does not have.)
#[must_use]
pub fn json<T: Serialize>(status: StatusCode, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => {
            let mut res = (status, body).into_response();
            res.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            res
        }
        Err(e) => {
            tracing::warn!(error = %e, "an API answer could not be serialised");
            (StatusCode::INTERNAL_SERVER_ERROR, "serialisation failed").into_response()
        }
    }
}

/// A refusal as a response.
#[must_use]
pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    json(
        status,
        &ApiError {
            code: code.to_owned(),
            message: message.to_owned(),
        },
    )
}

/// 503 `BUSY` with `Retry-After`: a stream over the cap ([`crate::limits::Limits::max_streams`]).
#[must_use]
pub fn busy(message: &str) -> Response {
    let mut res = error(StatusCode::SERVICE_UNAVAILABLE, "BUSY", message);
    res.headers_mut().insert(
        RETRY_AFTER,
        HeaderValue::from_static(crate::limits::RETRY_AFTER_SECS),
    );
    res
}

/// The answer to an `/api/` address no route has.
#[must_use]
pub fn not_found() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "The console has no API at this address.",
    )
}

/// The value of `name` in a query string, as written (the API's own parameters are digits and dots: nothing to decode).
#[must_use]
pub fn param<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// The proof every API request must carry (see the module text).
pub async fn guard(req: Request, next: Next) -> Response {
    if let Some(refused) = refusal(&req) {
        return refused;
    }
    next.run(req).await
}

/// Why [`guard`] refuses a request, if it does.
fn refusal(req: &Request) -> Option<Response> {
    let header = |name: &HeaderName| req.headers().get(name).and_then(|v| v.to_str().ok());
    if header(&HeaderName::from_static(PROOF_HEADER)) != Some("1") {
        return Some(error(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "The console's API answers only its own pages, which send the header X-Console: 1.",
        ));
    }
    if let Some(site) = header(&FETCH_SITE)
        && !site.trim().eq_ignore_ascii_case("same-origin")
    {
        return Some(error(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            origin::Refusal::CrossSite.words(),
        ));
    }
    origin::check(
        req.method().as_str(),
        header(&ORIGIN),
        header(&FETCH_SITE),
        header(&HOST),
    )
    .err()
    .map(|why| error(StatusCode::FORBIDDEN, "FORBIDDEN", why.words()))
}

#[cfg(test)]
mod tests {
    use super::param;

    #[test]
    fn a_parameter_is_found_by_its_whole_name() {
        assert_eq!(param(Some("since=12.5&limit=3"), "since"), Some("12.5"));
        assert_eq!(param(Some("since=12.5&limit=3"), "limit"), Some("3"));
        assert_eq!(param(Some("xsince=1"), "since"), None);
        assert_eq!(param(Some("since"), "since"), None);
        assert_eq!(param(None, "since"), None);
    }
}
