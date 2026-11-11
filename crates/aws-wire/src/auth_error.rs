//! Authentication failures, worded the way each protocol family words them.
//!
//! **Unverified against AWS.** The codes and statuses follow the Go edition and the public documentation; the spike that wrote
//! this had no account to confirm them. They are on the recording queue (a raw-HTTP scenario for each failure, in each family)
//! and a test pins the table so a change is deliberate.

use aws_auth::AuthError;
use aws_core::time::{format_amz_date, format_iso8601};

use crate::ApiError;

/// Which family of error vocabulary applies to a protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// The JSON, CBOR and restJson services (the "Coral" front end).
    Coral,
    /// awsQuery.
    Query,
    /// S3, which has its own.
    S3,
}

const MISMATCH_MSG: &str = "The request signature we calculated does not match the signature you provided. Check your AWS Secret Access Key and signing method. Consult the service documentation for details.";
const BAD_TOKEN_MSG: &str = "The security token included in the request is invalid.";

fn e(status: u16, code: &'static str, message: impl Into<String>) -> ApiError {
    ApiError::new(status, code, message)
}

/// The wire error for an authentication failure.
#[must_use]
pub fn from_auth(err: &AuthError, family: Family) -> ApiError {
    use Family::{Coral, Query, S3};
    match (err, family) {
        (AuthError::Missing, Coral) => e(400, "MissingAuthenticationToken", "Missing Authentication Token"),
        (AuthError::Missing, Query) => e(403, "MissingAuthenticationToken", "Request is missing Authentication Token"),
        (AuthError::Missing, S3) => e(403, "AccessDenied", "Access Denied"),
        (AuthError::Malformed(why), Coral | Query) => e(403, "IncompleteSignature", format!("Authorization header is malformed: {why}")),
        (AuthError::Malformed(why), S3) => e(400, "AuthorizationHeaderMalformed", format!("The authorization header is malformed; {why}")),
        (AuthError::UnsupportedAlgorithm, S3) => e(400, "InvalidRequest", "The authorization mechanism you have provided is not supported. Please use AWS4-HMAC-SHA256."),
        (AuthError::UnsupportedAlgorithm, _) => e(403, "IncompleteSignature", "Unsupported AWS 'algorithm': use AWS4-HMAC-SHA256"),
        (AuthError::UnknownAccessKey(_) | AuthError::InvalidToken, Coral) => e(400, "UnrecognizedClientException", BAD_TOKEN_MSG),
        (AuthError::UnknownAccessKey(_) | AuthError::InvalidToken, Query) => e(403, "InvalidClientTokenId", BAD_TOKEN_MSG),
        (AuthError::UnknownAccessKey(k), S3) => e(403, "InvalidAccessKeyId", "The AWS Access Key Id you provided does not exist in our records.").with("AWSAccessKeyId", k.as_str()),
        (AuthError::InvalidToken, S3) => e(400, "InvalidToken", "The provided token is malformed or otherwise invalid."),
        (AuthError::ExpiredToken, Coral) => e(400, "ExpiredTokenException", "The security token included in the request is expired"),
        (AuthError::ExpiredToken, Query) => e(403, "ExpiredToken", "The security token included in the request is expired"),
        (AuthError::ExpiredToken, S3) => e(400, "ExpiredToken", "The provided token has expired."),
        (AuthError::ScopeMismatch { scope, expected }, f) => scoped(f, format!("Credential should be scoped to correct service: '{expected}' (got '{scope}').")),
        (AuthError::MissingSignedHeader(h), f) => scoped(f, format!("SignedHeaders names '{h}' but the request has no such header.")),
        (AuthError::Skewed { request_time, server_time, max, .. }, f) => skewed(f, *request_time, *server_time, *max),
        (AuthError::PresignExpired { request_time, expires_in, server_time }, S3) => e(403, "AccessDenied", "Request has expired")
            .with("X-Amz-Expires", expires_in.to_string())
            .with("Expires", format_iso8601(request_time + expires_in))
            .with("ServerTime", format_iso8601(*server_time)),
        (AuthError::PresignExpired { .. }, Query) => e(403, "ExpiredToken", "Request has expired."),
        (AuthError::PresignExpired { .. }, Coral) => e(400, "ExpiredTokenException", "Request has expired."),
        (AuthError::BadExpires, S3) => e(400, "AuthorizationQueryParametersError", "X-Amz-Expires must be a number between 1 and 604800 seconds"),
        (AuthError::BadExpires, _) => e(403, "IncompleteSignature", "X-Amz-Expires must be a number between 1 and 604800 seconds"),
        (AuthError::BadPayloadHash(v), S3) => e(400, "InvalidArgument", format!("x-amz-content-sha256 must be UNSIGNED-PAYLOAD, STREAMING-UNSIGNED-PAYLOAD-TRAILER, STREAMING-AWS4-HMAC-SHA256-PAYLOAD, STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER or a valid sha256 value (got {v:?}).")),
        (AuthError::BadPayloadHash(v), _) => e(400, "InvalidSignatureException", format!("invalid x-amz-content-sha256 {v:?}")),
        (AuthError::PayloadHashMismatch, S3) => e(400, "XAmzContentSHA256Mismatch", "The provided 'x-amz-content-sha256' header does not match what was computed."),
        (AuthError::PayloadHashMismatch, _) => e(400, "InvalidSignatureException", MISMATCH_MSG),
        (AuthError::SignatureMismatch { access_key_id, signature_provided, canonical_request, string_to_sign }, S3) => {
            e(403, "SignatureDoesNotMatch", "The request signature we calculated does not match the signature you provided. Check your key and signing method.")
                .with("AWSAccessKeyId", access_key_id.as_str())
                .with("StringToSign", string_to_sign.as_str())
                .with("SignatureProvided", signature_provided.as_str())
                .with("CanonicalRequest", canonical_request.as_str())
        }
        (AuthError::SignatureMismatch { .. }, Query) => e(403, "SignatureDoesNotMatch", MISMATCH_MSG),
        (AuthError::SignatureMismatch { .. }, Coral) => e(400, "InvalidSignatureException", MISMATCH_MSG),
        (AuthError::Internal, _) => ApiError::internal("signing failure"),
        _ => ApiError::internal("authentication failed"),
    }
}

/// The "signature does not match" family: S3 and Query call it that, Coral calls it invalid.
fn scoped(family: Family, message: String) -> ApiError {
    match family {
        Family::Coral => ApiError::new(400, "InvalidSignatureException", message),
        Family::Query | Family::S3 => ApiError::new(403, "SignatureDoesNotMatch", message),
    }
}

fn skewed(family: Family, request_time: i64, server_time: i64, max: i64) -> ApiError {
    if family == Family::S3 {
        return ApiError::new(
            403,
            "RequestTimeTooSkewed",
            "The difference between the request time and the current time is too large.",
        )
        .with("RequestTime", format_iso8601(request_time))
        .with("ServerTime", format_iso8601(server_time))
        .with("MaxAllowedSkewMilliseconds", (max * 1000).to_string())
        .with("Max", max.to_string());
    }
    let (lo, hi) = (request_time.min(server_time), request_time.max(server_time));
    let message = format!(
        "Signature expired: {} is now earlier than {} ({} - {} min.)",
        format_amz_date(lo),
        format_amz_date(server_time.saturating_sub(max)),
        format_amz_date(hi),
        max / 60
    );
    match family {
        Family::Query => ApiError::new(403, "SignatureDoesNotMatch", message),
        Family::Coral | Family::S3 => ApiError::new(400, "InvalidSignatureException", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_failure_is_worded_per_family() {
        let unknown = AuthError::UnknownAccessKey("AKIA".into());
        assert_eq!(
            from_auth(&unknown, Family::Coral).code,
            "UnrecognizedClientException"
        );
        assert_eq!(
            from_auth(&unknown, Family::Query).code,
            "InvalidClientTokenId"
        );
        let s3 = from_auth(&unknown, Family::S3);
        assert_eq!((s3.status, s3.code.as_ref()), (403, "InvalidAccessKeyId"));
        assert_eq!(s3.extra, [("AWSAccessKeyId".to_owned(), "AKIA".to_owned())]);
    }

    #[test]
    fn a_skewed_clock_reports_the_window() {
        let e = from_auth(
            &AuthError::Skewed {
                request_time: 1_369_353_600,
                server_time: 1_369_354_600,
                skew: 1000,
                max: 300,
            },
            Family::Coral,
        );
        assert_eq!(
            (e.status, e.code.as_ref()),
            (400, "InvalidSignatureException")
        );
        assert!(
            e.message.starts_with(
                "Signature expired: 20130524T000000Z is now earlier than 20130524T001140Z"
            ),
            "{}",
            e.message
        );
        let s3 = from_auth(
            &AuthError::Skewed {
                request_time: 0,
                server_time: 1000,
                skew: 1000,
                max: 900,
            },
            Family::S3,
        );
        assert_eq!(s3.code, "RequestTimeTooSkewed");
        assert!(
            s3.extra
                .contains(&("MaxAllowedSkewMilliseconds".to_owned(), "900000".to_owned()))
        );
    }
}
