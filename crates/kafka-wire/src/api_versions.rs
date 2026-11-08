//! Answering ApiVersions: the one response that defines the whole compatibility envelope.
//!
//! Two answers are not the list, and both are Apache Kafka's (recorded against 4.3.1):
//!
//! - A version of ApiVersions this broker does not serve is answered in the v0 form, the only one a
//!   client is sure to read, with UNSUPPORTED_VERSION and one row: the versions of ApiVersions itself. The
//!   client asks again at one of those. The body of such a request is not read.
//! - From v3 a client says what it is. A software name or version that is empty, or is not letters and
//!   digits with dots and dashes between them, is INVALID_REQUEST with no rows; the connection lives on.

use kafka_protocol::ResponseError;
use kafka_protocol::messages::api_versions_response::ApiVersion;
use kafka_protocol::messages::{ApiKey, ApiVersionsRequest, ApiVersionsResponse};

use crate::profile::{Features, Profile};

/// Whether a client software name or version is well formed: ASCII letters and digits, with dots and dashes
/// allowed between them.
#[must_use]
pub fn valid_client_software(s: &str) -> bool {
    let alnum = |c: u8| c.is_ascii_alphanumeric();
    let b = s.as_bytes();
    match (b.first(), b.last()) {
        (Some(&first), Some(&last)) if alnum(first) && alnum(last) => {
            b.iter().all(|&c| alnum(c) || c == b'-' || c == b'.')
        }
        _ => false,
    }
}

/// The answer to an ApiVersions request at `version`, and the version it must be encoded at (v0 when the
/// request's own version is not served). `request` is the decoded body, if there was a readable one.
#[must_use]
pub fn answer(
    profile: Profile,
    features: Features,
    version: i16,
    request: Option<&ApiVersionsRequest>,
) -> (i16, ApiVersionsResponse) {
    let (min, max) = profile.range(ApiKey::ApiVersions).unwrap_or((0, 0));
    let mut resp = ApiVersionsResponse::default();
    if !(min..=max).contains(&version) {
        resp.error_code = ResponseError::UnsupportedVersion.code();
        resp.api_keys.push(row(ApiKey::ApiVersions, min, max));
        return (0, resp);
    }
    if version >= 3 && request.is_some_and(|q| !named_well(q)) {
        resp.error_code = ResponseError::InvalidRequest.code();
        return (version, resp);
    }
    resp.api_keys = profile
        .advertised(features)
        .into_iter()
        .map(|a| row(a.key, a.min, a.max))
        .collect();
    (version, resp)
}

fn named_well(q: &ApiVersionsRequest) -> bool {
    valid_client_software(&q.client_software_name)
        && valid_client_software(&q.client_software_version)
}

fn row(key: ApiKey, min: i16, max: i16) -> ApiVersion {
    let mut r = ApiVersion::default();
    r.api_key = key as i16;
    r.min_version = min;
    r.max_version = max;
    r
}

#[cfg(test)]
mod tests;
