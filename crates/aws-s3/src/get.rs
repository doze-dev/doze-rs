//! Reading an object: which version, whether the conditions hold, which bytes, and the headers that describe them.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::errors::{
    invalid_range, no_such_key, no_such_version, not_modified, precondition_failed,
};
use crate::model::{Seg, Version, Versioning};
use crate::put::add_checksum;
use crate::req::{Answer, Out, Req, Slice};
use crate::timeutil::parse_http_date;

/// What a `Range` header asks of an object of a given size.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Span {
    /// All of it (no range, or one S3 ignores: several ranges, or a malformed one).
    Whole,
    /// These bytes, both ends included.
    Bytes(u64, u64),
    /// Nothing of the object lies in the range.
    Unsatisfiable,
}

pub(crate) fn parse_range(header: &str, size: u64) -> Span {
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return Span::Whole;
    };
    if spec.contains(',') {
        return Span::Whole;
    }
    let Some((from, to)) = spec.split_once('-') else {
        return Span::Whole;
    };
    match (from.trim().parse::<u64>(), to.trim().parse::<u64>()) {
        (Ok(a), Ok(b)) if b >= a => bounded(a, b.min(size.saturating_sub(1)), size),
        (Ok(a), Err(_)) if to.trim().is_empty() => bounded(a, size.saturating_sub(1), size),
        (Err(_), Ok(n)) if from.trim().is_empty() => {
            if n == 0 || size == 0 {
                Span::Unsatisfiable
            } else {
                Span::Bytes(size.saturating_sub(n), size - 1)
            }
        }
        _ => Span::Whole,
    }
}

fn bounded(a: u64, b: u64, size: u64) -> Span {
    if a >= size {
        Span::Unsatisfiable
    } else {
        Span::Bytes(a, b)
    }
}

/// The slices of `segs` that make up bytes `from` to `from + len`.
pub(crate) fn slices(
    core: &Core,
    segs: &[Seg],
    from: u64,
    len: u64,
) -> Result<Vec<Slice>, ApiError> {
    let mut out = Vec::new();
    let (mut at, end) = (0u64, from + len);
    for seg in segs {
        let (lo, hi) = (at.max(from), (at + seg.len).min(end));
        if lo < hi {
            let file = core
                .blobs
                .open(&seg.blob)
                .map_err(|e| ApiError::internal(format!("an object's bytes are missing: {e}")))?;
            out.push(Slice {
                file: std::sync::Arc::new(file),
                from: lo - at,
                len: hi - lo,
            });
        }
        at += seg.len;
    }
    Ok(out)
}

impl Core {
    /// The version a read names: the one given, else the newest; a delete marker is not an object.
    pub(crate) fn readable(
        &self,
        bucket: &str,
        key: &str,
        id: Option<&str>,
    ) -> Result<Version, ApiError> {
        self.require(bucket)?;
        if let Some(id) = id {
            let v = self
                .find(bucket, key, id)?
                .ok_or_else(|| no_such_version(key, id))?;
            if v.delete_marker {
                return Err(ApiError::new(
                    405,
                    "MethodNotAllowed",
                    "The specified method is not allowed against this resource.",
                )
                .with("Method", "GET")
                .with("Key", key)
                .with("VersionId", id));
            }
            return Ok(v);
        }
        self.latest(bucket, key)?
            .filter(|v| !v.delete_marker)
            .ok_or_else(|| no_such_key(key))
    }

    pub(crate) fn get_object(&self, req: &Req<'_>, head: bool) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        let v = self.readable(bucket, key, req.s("VersionId"))?;
        if !head {
            check_readable(&v)?;
        }
        check_conditions(req, &v)?;
        let (span, part) = self.span_of(req, &v)?;
        let (from, len) = match span {
            Span::Whole => (0, v.size),
            Span::Bytes(a, b) => (a, b - a + 1),
            Span::Unsatisfiable => {
                return Err(invalid_range(v.size, req.s("Range").unwrap_or_default()));
            }
        };
        let ranged = !matches!(span, Span::Whole) || part.is_some();
        let mut out = self.object_headers(
            bucket,
            &v,
            len == v.size && req.s("ChecksumMode") == Some("ENABLED"),
        );
        out.insert("ContentLength".into(), json!(len));
        let status = if ranged {
            let last = (from + len).saturating_sub(1);
            out.insert(
                "ContentRange".into(),
                json!(format!("bytes {from}-{last}/{}", v.size)),
            );
            Some(206)
        } else {
            None
        };
        if part.is_some() {
            out.insert("PartsCount".into(), json!(v.parts.len()));
        }
        override_headers(req, &mut out);
        let body = slices(self, &v.segs, from, len)?;
        Ok(Out {
            json: Value::Object(out),
            status,
            body: Some(body),
        })
    }

    /// The bytes asked for, and the part number if one was asked for.
    fn span_of(&self, req: &Req<'_>, v: &Version) -> Result<(Span, Option<i64>), ApiError> {
        if let Some(n) = req.int("PartNumber") {
            let mut start = 0;
            for p in &v.parts {
                if i64::from(p.number) == n {
                    return Ok((Span::Bytes(start, start + p.size - 1), Some(n)));
                }
                start += p.size;
            }
            if v.parts.is_empty() && n == 1 {
                return Ok((Span::Whole, Some(n)));
            }
            return Err(invalid_range(v.size, &format!("partNumber={n}")));
        }
        Ok((
            req.s("Range")
                .map_or(Span::Whole, |r| parse_range(r, v.size)),
            None,
        ))
    }

    /// The headers that describe an object, as the output members that carry them. The checksum is included when the whole object
    /// is sent.
    pub(crate) fn object_headers(
        &self,
        bucket: &str,
        v: &Version,
        whole: bool,
    ) -> Map<String, Value> {
        let mut out = Map::new();
        out.insert("AcceptRanges".into(), json!("bytes"));
        out.insert("ETag".into(), json!(format!("\"{}\"", v.etag)));
        out.insert("LastModified".into(), json!(v.modified_ms / 1000));
        out.insert("ContentLength".into(), json!(v.size));
        let m = &v.meta;
        out.insert(
            "ContentType".into(),
            json!(m.content_type.as_deref().unwrap_or("binary/octet-stream")),
        );
        let optional = [
            ("CacheControl", &m.cache_control),
            ("ContentDisposition", &m.content_disposition),
            ("ContentEncoding", &m.content_encoding),
            ("ContentLanguage", &m.content_language),
            ("WebsiteRedirectLocation", &m.website_redirect),
            ("ServerSideEncryption", &m.sse),
            ("SSEKMSKeyId", &m.sse_kms_key),
        ];
        for (name, value) in optional {
            if let Some(x) = value {
                out.insert(name.into(), json!(x));
            }
        }
        if let Some(e) = &m.expires {
            out.insert("ExpiresString".into(), json!(e));
        }
        if let Some(sc) = m.storage_class.as_ref().filter(|s| *s != "STANDARD") {
            out.insert("StorageClass".into(), json!(sc));
        }
        out.insert(
            "Metadata".into(),
            Value::Object(m.user.iter().map(|(k, x)| (k.clone(), json!(x))).collect()),
        );
        if v.tag_count() > 0 {
            out.insert("TagCount".into(), json!(v.tag_count()));
        }
        let versioned = self
            .buckets
            .get(bucket)
            .is_some_and(|b| b.versioning != Versioning::Off);
        if versioned || v.version_id != "null" {
            out.insert("VersionId".into(), json!(v.version_id));
        }
        if whole {
            add_checksum(&mut out, v.checksum.as_ref());
        }
        crate::lock::describe(&v.lock, &mut out);
        out
    }
}

/// An object in an archive storage class cannot be read until restored; restoring is not built, so it cannot be read here.
pub(crate) fn check_readable(v: &Version) -> Result<(), ApiError> {
    if matches!(
        v.meta.storage_class.as_deref(),
        Some("GLACIER" | "DEEP_ARCHIVE")
    ) {
        return Err(ApiError::new(
            403,
            "InvalidObjectState",
            "The operation is not valid for the object's storage class",
        )
        .with(
            "StorageClass",
            v.meta.storage_class.clone().unwrap_or_default(),
        ));
    }
    Ok(())
}

/// `response-content-type` and the other query overrides of a read.
fn override_headers(req: &Req<'_>, out: &mut Map<String, Value>) {
    for (from, to) in [
        ("ResponseContentType", "ContentType"),
        ("ResponseCacheControl", "CacheControl"),
        ("ResponseContentDisposition", "ContentDisposition"),
        ("ResponseContentEncoding", "ContentEncoding"),
        ("ResponseContentLanguage", "ContentLanguage"),
        ("ResponseExpires", "ExpiresString"),
    ] {
        if let Some(x) = req.s(from) {
            out.insert(to.into(), json!(x));
        }
    }
}

/// `If-Match`, `If-None-Match`, `If-Modified-Since` and `If-Unmodified-Since`, in RFC 9110's order.
pub(crate) fn check_conditions(req: &Req<'_>, v: &Version) -> Result<(), ApiError> {
    let etag_in = |list: &str| {
        list.split(',')
            .map(|e| e.trim().trim_start_matches("W/").trim_matches('"'))
            .any(|e| e == "*" || e == v.etag)
    };
    let modified = v.modified_ms / 1000;
    let since = |name: &str| req.s(name).and_then(parse_http_date);
    if let Some(m) = req.s("IfMatch") {
        if !etag_in(m) {
            return Err(precondition_failed("If-Match"));
        }
    } else if since("IfUnmodifiedSince").is_some_and(|t| modified > t) {
        return Err(precondition_failed("If-Unmodified-Since"));
    }
    if let Some(n) = req.s("IfNoneMatch") {
        if etag_in(n) {
            return Err(not_modified());
        }
    } else if since("IfModifiedSince").is_some_and(|t| modified <= t) {
        return Err(not_modified());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_read_as_s3_reads_them() {
        let cases = [
            ("bytes=1-3", Span::Bytes(1, 3)),
            ("bytes=-5", Span::Bytes(6, 10)),
            ("bytes=6-", Span::Bytes(6, 10)),
            ("bytes=0-10", Span::Bytes(0, 10)),
            ("bytes=5-500", Span::Bytes(5, 10)),
            ("bytes=50-60", Span::Unsatisfiable),
            ("bytes=11-", Span::Unsatisfiable),
            ("bytes=0-1,4-5", Span::Whole),
            ("bytes=5-2", Span::Whole),
            ("lines=1-2", Span::Whole),
            ("bytes=-0", Span::Unsatisfiable),
        ];
        for (h, want) in cases {
            assert_eq!(parse_range(h, 11), want, "{h}");
        }
    }
}
