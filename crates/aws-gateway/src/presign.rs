//! Presigned URLs, as far as a gateway that holds no keys can judge them, and the query string they travel in.

use aws_auth::AuthError;
use aws_auth::claim::{Claim, Placement};
use aws_wire::{ApiError, Family, from_auth};
use http::request::Parts;

/// The claim of a SigV2 presigned URL (`AWSAccessKeyId`, `Expires`, `Signature`), as far as it means anything without the secret:
/// who it names, and that it expires at the absolute time it carries.
pub(crate) fn sigv2_claim(parts: &Parts) -> Option<Claim> {
    let q = pairs(parts.uri.query()?);
    let get = |name: &str| q.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let expires: i64 = get("Expires")?.parse().ok()?;
    let signature = get("Signature")?;
    Some(Claim {
        placement: Placement::Query,
        access_key_id: get("AWSAccessKeyId")?,
        scope_date: String::new(),
        region: "us-east-1".to_owned(),
        service: "s3".to_owned(),
        signed_headers: Vec::new(),
        signature: Vec::new(),
        signature_hex: signature,
        amz_date: String::new(),
        // The URL is good until `expires`: signed "at" that moment for a lifetime of nothing.
        request_time: expires,
        expires_in: Some(0),
        session_token: None,
        content_sha256: None,
    })
}

/// The claim of a request that carries no signature and is not for a JSON or Query service: an S3 request made with `curl` or a
/// browser. Served as the local account, by a gateway that trusts.
pub(crate) fn anonymous_claim(parts: &Parts) -> Option<Claim> {
    let h = &parts.headers;
    let for_another_protocol = h.contains_key("x-amz-target")
        || h.get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    if h.contains_key(http::header::AUTHORIZATION) || for_another_protocol {
        return None;
    }
    Some(Claim {
        placement: Placement::Header,
        access_key_id: "anonymous".to_owned(),
        scope_date: String::new(),
        region: "us-east-1".to_owned(),
        service: "s3".to_owned(),
        signed_headers: Vec::new(),
        signature: Vec::new(),
        signature_hex: String::new(),
        amz_date: String::new(),
        request_time: 0,
        expires_in: None,
        session_token: None,
        content_sha256: None,
    })
}

/// In trust mode the signature is not checked, but a presigned URL that has expired, or that names a signed header the request does
/// not carry, would be refused by S3 whatever the key, so it is refused here.
pub(crate) fn check_presigned(claim: &Claim, parts: &Parts) -> Result<(), ApiError> {
    if claim.placement != Placement::Query {
        return Ok(());
    }
    if let Some(expires_in) = claim.expires_in {
        let now = doze_runtime::Clock::now_ms(&doze_runtime::SystemClock) / 1000;
        if now > claim.request_time + expires_in {
            let err = AuthError::PresignExpired {
                request_time: claim.request_time,
                expires_in,
                server_time: now,
            };
            return Err(from_auth(&err, Family::S3));
        }
    }
    let missing = claim
        .signed_headers
        .iter()
        .find(|h| h.as_str() != "host" && !parts.headers.contains_key(h.as_str()));
    match missing {
        Some(h) => Err(from_auth(
            &AuthError::MissingSignedHeader(h.clone()),
            Family::S3,
        )),
        None => Ok(()),
    }
}

pub(crate) fn pairs(query: &str) -> Vec<(String, String)> {
    let decode = |s: &str| {
        let spaced = s.replace('+', " ");
        aws_core::uri::decode_str(&spaced).map_or(spaced.clone(), std::borrow::Cow::into_owned)
    };
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}
