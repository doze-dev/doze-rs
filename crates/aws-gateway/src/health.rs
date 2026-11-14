//! `GET /_doze/health`: what this process is, for a script, a supervisor or `doze-aws doctor`. A local hook, not an AWS address
//! (no bucket name can start with `_`, so path-style S3 never reaches it). Unsigned, and cheap: it answers from facts the gateway
//! was handed at start and wakes no service.

use http::request::Parts;
use http::{Method, Response, header};

use crate::body::RespBody;
use crate::pipeline::Shared;

/// The path.
pub(crate) const PATH: &str = "/_doze/health";

/// The answer, when the request is for the health path and the gateway was given facts to answer with.
pub(crate) fn answer(shared: &Shared, parts: &Parts) -> Option<Response<RespBody>> {
    if parts.uri.path() != PATH || !(parts.method == Method::GET || parts.method == Method::HEAD) {
        return None;
    }
    let mut facts = shared.config.health.clone()?;
    facts.insert(
        "uptime_secs".to_owned(),
        shared.started().elapsed().as_secs().into(),
    );
    let body = serde_json::to_vec(&serde_json::Value::Object(facts)).unwrap_or_default();
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(RespBody::whole(body))
        .ok()
}
