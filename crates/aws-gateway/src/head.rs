//! The head of a request: who is calling, its signature, its session token and endpoint, and reading (and inflating) its body.

use aws_auth::claim::Claim;
use aws_auth::{AuthError, Endpoint, begin};
use aws_wire::ApiError;
use aws_wire::service::Caller;
use bytes::Bytes;
use doze_runtime::{Clock, SystemClock};
use http::request::Parts;
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;

use crate::Auth;
use crate::pipeline::Shared;

/// Who is calling, from the credential scope and the request head.
pub(crate) fn caller_of(shared: &Shared, parts: &Parts, claim: Claim) -> Caller {
    Caller {
        access_key_id: claim.access_key_id,
        account_id: shared.config.account_id.clone(),
        region: claim.region,
        endpoint: endpoint_of(parts),
        service: None,
        session_token: session_token(parts),
    }
}

/// A body sent with `Content-Encoding: gzip` (botocore compresses `PutMetricData` past 10 KB, as the CloudWatch model says it may),
/// inflated. The signature covers the bytes as sent, so this comes after the check. What it inflates to is held to the same limit
/// as any body, so a small request cannot become a large one.
pub(crate) fn inflate(parts: &Parts, bytes: Bytes, limit: usize) -> Result<Bytes, ApiError> {
    use std::io::Read;
    let gzip = parts
        .headers
        .get(http::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("gzip"));
    if !gzip {
        return Ok(bytes);
    }
    let mut out = Vec::new();
    let read = flate2::read::GzDecoder::new(&bytes[..])
        .take(limit as u64 + 1)
        .read_to_end(&mut out);
    if read.is_err() {
        return Err(ApiError::new(
            400,
            "SerializationException",
            "The request body is not valid gzip.",
        ));
    }
    if out.len() > limit {
        return Err(ApiError::new(
            413,
            "RequestEntityTooLarge",
            format!("The request body exceeds {limit} bytes once inflated."),
        ));
    }
    Ok(Bytes::from(out))
}

/// The session token the request carried.
pub(crate) fn session_token(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("x-amz-security-token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// The scheme and host the caller used, for URLs in answers.
pub(crate) fn endpoint_of(parts: &Parts) -> String {
    let host = parts
        .headers
        .get(http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    format!("http://{host}")
}

/// Checks the head of the request in `Verify` mode; `None` in `Trust` mode.
pub(crate) async fn authenticate(
    shared: &Shared,
    parts: &Parts,
    signing_name: &str,
) -> Result<Option<aws_auth::Pending>, AuthError> {
    let Auth::Verify(creds) = &shared.config.auth else {
        return Ok(None);
    };
    let Some(entry) = shared.entry(signing_name) else {
        return Err(AuthError::Missing);
    };
    let endpoint = Endpoint {
        signing_name: entry.service.meta().signing_name,
        s3: signing_name == "s3",
        max_skew_secs: shared.config.max_skew_secs,
    };
    let now = SystemClock.now_ms() / 1000;
    // A key the fixed credentials do not hold may be one IAM issued: ask its source, which is only woken for keys that look issued.
    let claim = aws_auth::claim::Claim::parse(parts)?;
    let mut extra = None;
    if let (Some(claim), Some(source)) = (&claim, &shared.config.credential_source)
        && aws_auth::Credentials::lookup(creds.as_ref(), &claim.access_key_id, |c| c.is_none())
    {
        extra = source
            .find(&claim.access_key_id, claim.session_token.as_deref())
            .await
            .map(|c| (claim.access_key_id.clone(), c));
    }
    let with = aws_auth::WithExtra {
        fixed: creds.as_ref(),
        extra: extra.as_ref().map(|(id, c)| (id.as_str(), c)),
    };
    begin(parts, now, endpoint, &with).map(Some)
}

pub(crate) async fn read_body(body: Incoming, limit: usize) -> Result<Bytes, ApiError> {
    match Limited::new(body, limit).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(e) if e.is::<http_body_util::LengthLimitError>() => Err(ApiError::new(
            413,
            "RequestEntityTooLarge",
            format!("The request body exceeds {limit} bytes."),
        )),
        Err(e) => Err(ApiError::new(
            400,
            "IncompleteBody",
            format!("The request body could not be read: {e}"),
        )),
    }
}
