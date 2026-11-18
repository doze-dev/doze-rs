//! Logins, ACLs, SCRAM credentials, versions and telemetry. The SASL tokens, nonces, proofs, salts and salted passwords are
//! never read: a SaslAuthenticate row says only that a login step happened and how it ended; the credential APIs say the user
//! and the mechanism.

use kafka_wire::messages::{
    AlterUserScramCredentialsRequest, AlterUserScramCredentialsResponse, ApiVersionsRequest,
    ApiVersionsResponse, CreateAclsRequest, CreateAclsResponse, DeleteAclsRequest,
    DeleteAclsResponse, DescribeAclsRequest, DescribeAclsResponse,
    DescribeUserScramCredentialsRequest, DescribeUserScramCredentialsResponse,
    GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse, PushTelemetryRequest,
    PushTelemetryResponse, SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest,
    SaslHandshakeResponse,
};

use super::n;
use crate::recorder::clip;
use crate::recorder::facts::Facts;

pub(super) fn handshake_request(r: &SaslHandshakeRequest, f: &mut Facts) {
    f.about(&r.mechanism);
}

pub(super) fn handshake_response(r: &SaslHandshakeResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.mechanisms.len());
}

/// A login step. Nothing is read from the request: its bytes are the secret.
pub(super) fn authenticate_request(_: &SaslAuthenticateRequest, f: &mut Facts) {
    f.about("a login step");
}

/// The answer's own message is kept (the server's words, no secret); its `auth_bytes` are the server's side of the exchange and
/// are never read.
pub(super) fn authenticate_response(r: &SaslAuthenticateResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
}

pub(super) fn versions_request(r: &ApiVersionsRequest, f: &mut Facts) {
    f.about(&format!(
        "{} {}",
        clip::name(&r.client_software_name),
        clip::name(&r.client_software_version)
    ));
}

pub(super) fn versions_response(r: &ApiVersionsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.api_keys.len());
}

pub(super) fn describe_acls_request(_: &DescribeAclsRequest, f: &mut Facts) {
    f.about("a filter of ACLs");
}

pub(super) fn describe_acls_response(r: &DescribeAclsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    f.items = n(r.resources.iter().map(|x| x.acls.len()).sum());
}

pub(super) fn create_acls_request(r: &CreateAclsRequest, f: &mut Facts) {
    f.asked = n(r.creations.len());
    f.about("create ACLs");
}

pub(super) fn create_acls_response(r: &CreateAclsResponse, f: &mut Facts) {
    for c in &r.results {
        f.error(c.error_code);
        f.say(c.error_message.as_deref());
    }
    f.items = n(r.results.len());
}

pub(super) fn delete_acls_request(r: &DeleteAclsRequest, f: &mut Facts) {
    f.asked = n(r.filters.len());
    f.about("delete ACLs");
}

pub(super) fn delete_acls_response(r: &DeleteAclsResponse, f: &mut Facts) {
    let mut gone = 0usize;
    for res in &r.filter_results {
        f.error(res.error_code);
        f.say(res.error_message.as_deref());
        gone += res.matching_acls.len();
    }
    f.items = n(gone);
}

/// The names asked about: users, never credentials.
pub(super) fn describe_scram_request(r: &DescribeUserScramCredentialsRequest, f: &mut Facts) {
    f.asked = n(r.users.as_ref().map_or(0, Vec::len));
    f.about("SCRAM users");
}

pub(super) fn describe_scram_response(r: &DescribeUserScramCredentialsResponse, f: &mut Facts) {
    f.error(r.error_code);
    for u in &r.results {
        f.error(u.error_code);
    }
    f.items = n(r.results.len());
}

/// Only how many users are changed: not their names, mechanisms' salts or salted passwords.
pub(super) fn alter_scram_request(r: &AlterUserScramCredentialsRequest, f: &mut Facts) {
    f.asked = n(r.upsertions.len() + r.deletions.len());
    f.about("SCRAM credentials");
}

pub(super) fn alter_scram_response(r: &AlterUserScramCredentialsResponse, f: &mut Facts) {
    for u in &r.results {
        f.error(u.error_code);
        f.say(u.error_message.as_deref());
    }
    f.items = n(r.results.len());
}

pub(super) fn telemetry_get_request(_: &GetTelemetrySubscriptionsRequest, f: &mut Facts) {
    f.about("client metrics subscription");
}

pub(super) fn telemetry_get_response(r: &GetTelemetrySubscriptionsResponse, f: &mut Facts) {
    f.error(r.error_code);
}

/// The metrics payload is not read.
pub(super) fn telemetry_push_request(_: &PushTelemetryRequest, f: &mut Facts) {
    f.about("client metrics");
}

pub(super) fn telemetry_push_response(r: &PushTelemetryResponse, f: &mut Facts) {
    f.error(r.error_code);
}
