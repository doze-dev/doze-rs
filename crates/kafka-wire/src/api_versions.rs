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
use kafka_protocol::messages::api_versions_response::{
    ApiVersion, FinalizedFeatureKey, SupportedFeatureKey,
};
use kafka_protocol::messages::{ApiKey, ApiVersionsRequest, ApiVersionsResponse};
use kafka_protocol::protocol::StrBytes;

use crate::profile::{FeatureLevel, Features, Profile};

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
    if version >= 3 {
        for f in profile.features() {
            // A broker that does not serve share or streams groups says it supports neither (KIP-1022: off).
            let off = (f.name == "share.version" && !features.served.contains(ApiKey::ShareFetch))
                || (f.name == "streams.version"
                    && !features.served.contains(ApiKey::StreamsGroupHeartbeat));
            if !off {
                feature(f, version, &mut resp);
            }
        }
    }
    (version, resp)
}

/// One feature (KIP-584) as the recorded broker of the major lists it (`Profile::features`): every feature it supports,
/// with its levels, and, finalized, every one at a level above 0, with the finalized epoch not negative (a client takes the
/// features of an epoch newer than it has seen, from -1). Below v4 a supported feature whose least level is 0 is left out
/// (old clients cannot read it, KAFKA-17492). `transaction.version` is the one a client decides by: the Java client's
/// `TransactionManager` speaks KIP-890's second part at a finalized level of 2.
fn feature(f: FeatureLevel, version: i16, resp: &mut ApiVersionsResponse) {
    if version >= 4 || f.min > 0 {
        let mut supported = SupportedFeatureKey::default();
        supported.name = StrBytes::from_static_str(f.name);
        supported.min_version = f.min;
        supported.max_version = f.max;
        resp.supported_features.push(supported);
    }
    if f.level > 0 {
        let mut finalized = FinalizedFeatureKey::default();
        finalized.name = StrBytes::from_static_str(f.name);
        finalized.min_version_level = f.level;
        finalized.max_version_level = f.level;
        resp.finalized_features.push(finalized);
    }
    resp.finalized_features_epoch = 0;
}

/// The answer to an ApiVersions request that arrives in the middle of a SASL login: ILLEGAL_SASL_STATE and no
/// APIs, at the request's own version (v0 when that is not served), as Apache Kafka answers it. The request's
/// body is not needed, so a stranger's is never read.
#[must_use]
pub fn illegal_state(profile: Profile, version: i16) -> (i16, ApiVersionsResponse) {
    let (min, max) = profile.range(ApiKey::ApiVersions).unwrap_or((0, 0));
    let mut resp = ApiVersionsResponse::default();
    resp.error_code = ResponseError::IllegalSaslState.code();
    (
        if (min..=max).contains(&version) {
            version
        } else {
            0
        },
        resp,
    )
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
