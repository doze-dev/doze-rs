//! What a JSON request's `X-Amz-Target` says beyond the operation: which of two services that sign alike it is for, and an operation
//! a service adds to its model.

use http::request::Parts;

use crate::pipeline::Entry;

/// The prefix of the request's `X-Amz-Target` (`DynamoDBStreams_20120810`).
pub(crate) fn target_prefix(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .and_then(|t| t.split_once('.'))
        .map(|(prefix, _)| prefix)
}

/// The operation index and the borrowed shapes of an operation the service adds to its model, when the target names one.
pub(crate) fn extension(entry: &Entry, parts: &Parts) -> Option<(u32, u32)> {
    let target = parts.headers.get("x-amz-target")?.to_str().ok()?;
    let (prefix, name) = target.split_once('.')?;
    if parts.method != http::Method::POST
        || parts.uri.path() != "/"
        || prefix != entry.service.meta().shape_name
    {
        return None;
    }
    let ext = entry.service.extension(name)?;
    Some((ext.op, ext.shapes))
}
