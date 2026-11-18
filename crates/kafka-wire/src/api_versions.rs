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
    if version >= 3 {
        transaction_feature(profile.transaction_version(), version, &mut resp);
        let share = if features.served.contains(ApiKey::ShareFetch) {
            profile.share_version()
        } else {
            0
        };
        feature("share.version", share, 2, version, &mut resp);
        let streams = if features.served.contains(ApiKey::StreamsGroupHeartbeat) {
            profile.streams_version()
        } else {
            0
        };
        feature("streams.version", streams, 1, version, &mut resp);
    }
    (version, resp)
}

/// `share.version` (KIP-932) as Kafka 4.4.0 lists it when formatted with its defaults: supported 0 to 2, finalized at 2
/// (share groups on, level 1, and their dead-letter queue, level 2), under the same rules as `transaction.version`; and
/// `streams.version` (KIP-1071) likewise: supported 0 to 1, finalized at 1 (streams groups on). `most` is the highest
/// level supported.
fn feature(
    name: &'static str,
    level: i16,
    most: i16,
    version: i16,
    resp: &mut ApiVersionsResponse,
) {
    if level == 0 {
        return;
    }
    if version >= 4 {
        let mut supported = SupportedFeatureKey::default();
        supported.name = StrBytes::from_static_str(name);
        supported.min_version = 0;
        supported.max_version = most;
        resp.supported_features.push(supported);
    }
    let mut finalized = FinalizedFeatureKey::default();
    finalized.name = StrBytes::from_static_str(name);
    finalized.min_version_level = level;
    finalized.max_version_level = level;
    resp.finalized_features.push(finalized);
    resp.finalized_features_epoch = 0;
}

/// `transaction.version`, which a client reads to decide whether to speak KIP-890's second part (the Java client's
/// `TransactionManager` turns it on at a finalized level of 2). As Kafka 4.4.0 answers: supported 0 to 2 and finalized at
/// the level, with the finalized epoch not negative (the client takes the features of an epoch newer than it has seen,
/// from -1). Below v4 a supported feature whose least level is 0 is left out (old clients cannot read it, KAFKA-17492),
/// and a finalized level of 0 is never listed. Kafka lists its other features too (`metadata.version`, `group.version`,
/// ...); no client decides anything here by them, so they are not claimed. `share.version` and `streams.version` are the
/// others listed.
fn transaction_feature(level: i16, version: i16, resp: &mut ApiVersionsResponse) {
    feature("transaction.version", level, 2, version, resp);
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
