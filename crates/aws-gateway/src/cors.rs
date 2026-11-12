//! The CORS headers of a REST answer to a browser page: a request that carries `Origin` gets what the service says for its bucket
//! (S3's CORS rules), on success and on failure alike. The preflight itself is a [`Web`](aws_wire::service::Web) request.

use http::request::Parts;
use http::{HeaderValue, Response, header};

use crate::body::RespBody;
use crate::pipeline::Entry;
use crate::s3host::target;

/// Adds the service's CORS headers for the request's bucket to `response`.
pub(crate) async fn decorate(
    entry: &Entry,
    parts: &Parts,
    region: &str,
    response: &mut Response<RespBody>,
) {
    let (Some(origin), Some(rest)) = (
        parts
            .headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok()),
        entry.service.rest(),
    ) else {
        return;
    };
    let (path, _) = target(parts);
    let bucket = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default();
    if bucket.is_empty() {
        return;
    }
    for (k, v) in rest
        .cors(region, bucket, origin, parts.method.as_str())
        .await
    {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(&v),
        ) {
            response.headers_mut().insert(name, value);
        }
    }
}
