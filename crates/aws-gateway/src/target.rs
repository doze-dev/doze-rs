//! What a JSON request's `X-Amz-Target` says beyond the operation: which of two services that sign alike it is for.

use http::request::Parts;

/// The prefix of the request's `X-Amz-Target` (`DynamoDBStreams_20120810`).
pub(crate) fn target_prefix(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .and_then(|t| t.split_once('.'))
        .map(|(prefix, _)| prefix)
}
