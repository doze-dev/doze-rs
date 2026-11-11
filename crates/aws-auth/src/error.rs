//! Why a request failed authentication. How each protocol family words these on the wire is `aws-wire`'s business; this is the
//! reason, with everything an error body might need.

/// An authentication failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// The request carries no authentication material at all.
    #[error("no authentication material in the request")]
    Missing,
    /// There is signature material, and it is unusable.
    #[error("malformed authorization: {0}")]
    Malformed(&'static str),
    /// A signing algorithm other than `AWS4-HMAC-SHA256` (SigV2 included).
    #[error("unsupported signing algorithm")]
    UnsupportedAlgorithm,
    /// No credential has this access key id.
    #[error("unknown access key id {0}")]
    UnknownAccessKey(String),
    /// A session credential with a missing or wrong security token.
    #[error("security token missing or wrong for session credential")]
    InvalidToken,
    /// A session credential past its expiry.
    #[error("session credential expired")]
    ExpiredToken,
    /// The credential scope names another service than the endpoint's.
    #[error("credential scope names service {scope:?}, endpoint is {expected:?}")]
    ScopeMismatch {
        /// The service in the credential scope.
        scope: String,
        /// The service this endpoint signs for.
        expected: &'static str,
    },
    /// A header the client signed is not in the request.
    #[error("signed header {0:?} is not present in the request")]
    MissingSignedHeader(String),
    /// The request time is too far from the server's.
    #[error("request time {request_time} is {skew}s from server time {server_time}")]
    Skewed {
        /// The time the client signed, in Unix seconds.
        request_time: i64,
        /// The server's time.
        server_time: i64,
        /// The absolute difference.
        skew: i64,
        /// The most the endpoint allows.
        max: i64,
    },
    /// A presigned request past its lifetime.
    #[error(
        "presigned request ({expires_in}s lifetime from {request_time}) expired at server time {server_time}"
    )]
    PresignExpired {
        /// When the URL was signed.
        request_time: i64,
        /// Its lifetime in seconds.
        expires_in: i64,
        /// The server's time.
        server_time: i64,
    },
    /// `X-Amz-Expires` is not a number from 1 to 604,800.
    #[error("presigned X-Amz-Expires out of range")]
    BadExpires,
    /// `x-amz-content-sha256` is neither a known mode nor 64 hex digits.
    #[error("invalid x-amz-content-sha256 {0:?}")]
    BadPayloadHash(String),
    /// The body does not hash to the `x-amz-content-sha256` the client signed.
    #[error("payload hash does not match x-amz-content-sha256")]
    PayloadHashMismatch,
    /// The signature is wrong. Both strings are kept: S3 puts them in its error body.
    #[error("signature mismatch")]
    SignatureMismatch {
        /// The access key id that signed.
        access_key_id: String,
        /// The signature the client sent, as hex.
        signature_provided: String,
        /// The canonical request the server rebuilt.
        canonical_request: String,
        /// The string to sign the server derived.
        string_to_sign: String,
    },
    /// The key could not be derived (an internal fault, not the client's).
    #[error("internal signing failure")]
    Internal,
}
