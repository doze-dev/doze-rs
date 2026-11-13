//! Pull a SigV4 claim out of an `Authorization` header or a presigned query string.
//! Nothing here trusts the claim; [`crate::verify`] does that.
use crate::error::AuthError;
use aws_core::{hex, time::parse_amz_date, uri};
use http::request::Parts;
use std::borrow::Cow;

/// The only algorithm we accept.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Where the signature travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Header,
    Query,
}

/// Everything a request claims about its own signature.
#[derive(Debug)]
pub struct Claim {
    pub placement: Placement,
    pub access_key_id: String,
    /// Credential-scope date, `YYYYMMDD`.
    pub scope_date: String,
    pub region: String,
    pub service: String,
    /// Lower-case names exactly as the client listed them.
    pub signed_headers: Vec<String>,
    pub signature: Vec<u8>,
    /// The signature as the client wrote it (hex), for error bodies.
    pub signature_hex: String,
    /// Request timestamp as sent (`YYYYMMDDTHHMMSSZ`) and parsed.
    pub amz_date: String,
    pub request_time: i64,
    /// Presigned lifetime in seconds.
    pub expires_in: Option<i64>,
    pub session_token: Option<String>,
    /// Raw `x-amz-content-sha256` header / `X-Amz-Content-Sha256` query value.
    pub content_sha256: Option<String>,
}

impl Claim {
    // fuzz:parser fuzz_claim
    /// `Ok(None)` means the request carries no signature material at all.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] and friends when material is present but unusable.
    pub fn parse(parts: &Parts) -> Result<Option<Self>, AuthError> {
        if let Some(h) = parts.headers.get(http::header::AUTHORIZATION) {
            let h = h
                .to_str()
                .map_err(|_| AuthError::Malformed("Authorization is not ASCII"))?;
            return Self::from_header(parts, h).map(Some);
        }
        if let Some(q) = parts.uri.query() {
            let has = |prefix: &str| q.split('&').any(|p| p.starts_with(prefix));
            if has("X-Amz-Algorithm=") || has("X-Amz-Signature=") {
                return Self::from_query(q).map(Some);
            }
            // SigV2 presigned URLs (boto3's default `generate_presigned_url` for S3).
            if has("AWSAccessKeyId=") && has("Signature=") {
                return Err(AuthError::UnsupportedAlgorithm);
            }
        }
        Ok(None)
    }

    fn from_header(parts: &Parts, header: &str) -> Result<Self, AuthError> {
        let rest = header
            .trim()
            .strip_prefix(ALGORITHM)
            .ok_or(AuthError::UnsupportedAlgorithm)?;
        let (mut cred, mut signed, mut sig) = (None, None, None);
        for field in rest.split(',') {
            match field.trim().split_once('=') {
                Some(("Credential", v)) => cred = Some(v),
                Some(("SignedHeaders", v)) => signed = Some(v),
                Some(("Signature", v)) => sig = Some(v),
                _ => {}
            }
        }
        let cred = cred.ok_or(AuthError::Malformed("no Credential"))?;
        let signed = signed.ok_or(AuthError::Malformed("no SignedHeaders"))?;
        let sig = sig.ok_or(AuthError::Malformed("no Signature"))?;
        let amz_date = parts
            .headers
            .get("x-amz-date")
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::Malformed("no x-amz-date header"))?;
        let header_str = |name: &str| {
            parts
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        Self::build(
            Placement::Header,
            cred,
            signed,
            sig,
            amz_date,
            None,
            header_str("x-amz-security-token"),
            header_str("x-amz-content-sha256"),
        )
    }

    fn from_query(query: &str) -> Result<Self, AuthError> {
        let (mut alg, mut cred, mut date, mut expires, mut signed, mut sig, mut token, mut sha) =
            (None, None, None, None, None, None, None, None);
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            let slot: &mut Option<Cow<'_, str>> = match &*k {
                "X-Amz-Algorithm" => &mut alg,
                "X-Amz-Credential" => &mut cred,
                "X-Amz-Date" => &mut date,
                "X-Amz-Expires" => &mut expires,
                "X-Amz-SignedHeaders" => &mut signed,
                "X-Amz-Signature" => &mut sig,
                "X-Amz-Security-Token" => &mut token,
                "X-Amz-Content-Sha256" => &mut sha,
                _ => continue,
            };
            *slot = Some(v);
        }
        if alg.as_deref() != Some(ALGORITHM) {
            return Err(AuthError::UnsupportedAlgorithm);
        }
        let expires_in = expires
            .as_deref()
            .ok_or(AuthError::BadExpires)?
            .parse::<i64>()
            .map_err(|_| AuthError::BadExpires)?;
        if !(1..=604_800).contains(&expires_in) {
            return Err(AuthError::BadExpires);
        }
        Self::build(
            Placement::Query,
            cred.as_deref()
                .ok_or(AuthError::Malformed("no X-Amz-Credential"))?,
            signed
                .as_deref()
                .ok_or(AuthError::Malformed("no X-Amz-SignedHeaders"))?,
            sig.as_deref()
                .ok_or(AuthError::Malformed("no X-Amz-Signature"))?,
            date.as_deref()
                .ok_or(AuthError::Malformed("no X-Amz-Date"))?,
            Some(expires_in),
            token.map(Cow::into_owned),
            sha.map(Cow::into_owned),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        placement: Placement,
        cred: &str,
        signed: &str,
        sig: &str,
        amz_date: &str,
        expires_in: Option<i64>,
        session_token: Option<String>,
        content_sha256: Option<String>,
    ) -> Result<Self, AuthError> {
        // Credential=AKID/20130524/us-east-1/s3/aws4_request. The access key id
        // is not allowed to contain '/', the other parts never do.
        let mut it = cred.splitn(5, '/');
        let (Some(akid), Some(date), Some(region), Some(service), Some("aws4_request")) =
            (it.next(), it.next(), it.next(), it.next(), it.next())
        else {
            return Err(AuthError::Malformed(
                "Credential must be AKID/date/region/service/aws4_request",
            ));
        };
        // A region names a directory in the services behind the gateway: lower-case letters, digits and dashes only.
        if region.is_empty()
            || region.len() > 32
            || !region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(AuthError::Malformed(
                "Credential should be scoped to a valid region.",
            ));
        }
        let request_time = parse_amz_date(amz_date)
            .ok_or(AuthError::Malformed("x-amz-date is not YYYYMMDDTHHMMSSZ"))?;
        let signature = hex::decode(sig).ok_or(AuthError::Malformed("Signature is not hex"))?;
        let signed_headers: Vec<String> = signed
            .split(';')
            .filter(|h| !h.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        if signed_headers.is_empty() {
            return Err(AuthError::Malformed("SignedHeaders is empty"));
        }
        // The scope date must agree with the request timestamp.
        if amz_date.get(..8) != Some(date) {
            return Err(AuthError::Malformed(
                "credential date does not match x-amz-date",
            ));
        }
        Ok(Self {
            placement,
            access_key_id: akid.to_owned(),
            scope_date: date.to_owned(),
            region: region.to_owned(),
            service: service.to_owned(),
            signed_headers,
            signature,
            signature_hex: sig.to_owned(),
            amz_date: amz_date.to_owned(),
            request_time,
            expires_in,
            session_token,
            content_sha256,
        })
    }

    /// `date/region/service/aws4_request`.
    #[must_use]
    pub fn scope(&self) -> String {
        format!(
            "{}/{}/{}/aws4_request",
            self.scope_date, self.region, self.service
        )
    }
}

/// How the request body relates to the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadHash {
    /// No `x-amz-content-sha256`: the hash is over the body, which must be buffered first.
    Absent,
    /// `UNSIGNED-PAYLOAD`.
    Unsigned,
    /// A claimed hex SHA-256 the body must match.
    Hex([u8; 32]),
    /// One of the `STREAMING-*` modes (aws-chunked body).
    Streaming(StreamKind),
}

/// The aws-chunked flavours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// `STREAMING-AWS4-HMAC-SHA256-PAYLOAD`: every chunk signed.
    Signed,
    /// `STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER`: chunks and trailer signed.
    SignedTrailer,
    /// `STREAMING-UNSIGNED-PAYLOAD-TRAILER`: unsigned chunks, trailing checksum.
    UnsignedTrailer,
}

impl PayloadHash {
    /// Classify the claimed hash. Presigned requests default to `UNSIGNED-PAYLOAD`.
    ///
    /// # Errors
    /// [`AuthError::BadPayloadHash`] for anything that is not a known mode or 64 hex digits.
    pub fn classify(
        claimed: Option<&str>,
        presigned: bool,
    ) -> Result<(Self, Option<&str>), AuthError> {
        let Some(v) = claimed else {
            return Ok(if presigned {
                (Self::Unsigned, Some("UNSIGNED-PAYLOAD"))
            } else {
                (Self::Absent, None)
            });
        };
        let kind = match v {
            "UNSIGNED-PAYLOAD" => Self::Unsigned,
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD" => Self::Streaming(StreamKind::Signed),
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER" => {
                Self::Streaming(StreamKind::SignedTrailer)
            }
            "STREAMING-UNSIGNED-PAYLOAD-TRAILER" => Self::Streaming(StreamKind::UnsignedTrailer),
            hexed => {
                let arr = hex::decode(hexed)
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                    .ok_or_else(|| AuthError::BadPayloadHash(hexed.to_owned()))?;
                Self::Hex(arr)
            }
        };
        Ok((kind, Some(v)))
    }
}

/// Strip an optional `:port` and lower-case: the host part of a `Host` header.
#[must_use]
pub fn host_without_port(host: &str) -> &str {
    if host.starts_with('[') {
        return host
            .split_once(']')
            .map_or(host, |(h, _)| h.trim_start_matches('['));
    }
    host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            h
        } else {
            host
        }
    })
}

/// Percent-decoded path as UTF-8 (used by REST routers, not by signing).
#[must_use]
pub fn decoded_path(path: &str) -> Option<Cow<'_, str>> {
    uri::decode_str(path)
}
