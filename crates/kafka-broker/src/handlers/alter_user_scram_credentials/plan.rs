//! What an AlterUserScramCredentials request comes to: the rules of `ScramControlManager.alterCredentials` (Kafka
//! 4.3.1, `ScramControlManager.java:179-257`) as a function of the request and of who exists, with no I/O.

use std::num::NonZeroU32;

use kafka_auth::scram::{Kind, Verifier};
use kafka_wire::ResponseError;
use kafka_wire::messages::AlterUserScramCredentialsRequest;
use kafka_wire::messages::alter_user_scram_credentials_request::ScramCredentialUpsertion;

use crate::users::kind_of;

/// `MAX_ITERATIONS` (`ScramControlManager.java:58`); the least is 4096 for both mechanisms (`ScramMechanism.java:32-33`).
const MIN_ITERATIONS: i32 = 4096;
const MAX_ITERATIONS: i32 = 16384;

/// Why a user's operation did not happen: Kafka's error code and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) code: i16,
    pub(super) message: Option<&'static str>,
}

const fn failure(code: ResponseError, message: &'static str) -> Failure {
    Failure {
        code: code.code(),
        message: Some(message),
    }
}

const TWICE: Failure = failure(
    ResponseError::DuplicateResource,
    "A user credential cannot be altered twice in the same request",
);
/// Kafka has no users configured outside the quorum, so no code of its own fits; a credential it will not accept is
/// UNACCEPTABLE_CREDENTIAL (SEC-D50).
pub(super) const CONFIGURED: Failure = failure(
    ResponseError::UnacceptableCredential,
    "The user is configured at start (--sasl-user, --sasl-users-file or DOZE_KAFKA_SASL_USERS) and cannot be changed over the API",
);
/// A super user's credential asked for by a caller who is not one: CLUSTER_AUTHORIZATION_FAILED, with no message, as
/// the refusal of the whole request is (SEC-190, SEC-D70).
pub(super) const SUPER_USER: Failure = Failure {
    code: ResponseError::ClusterAuthorizationFailed.code(),
    message: None,
};

/// What is to be done, and what is refused. Each list is in the order the request gave the operations.
#[derive(Debug)]
pub(super) struct Plan {
    /// The credentials to remove.
    pub(super) deletions: Vec<(String, Kind)>,
    /// The credentials to set, as they will be kept.
    pub(super) upsertions: Vec<(String, Verifier)>,
    /// The users whose operation was refused, in the order they were found out.
    pub(super) failures: Vec<(String, Failure)>,
}

/// Judges `request`. `refused` says why a user may not be changed (configured at start, or a super user and the caller
/// is not one), `stored` whether a stored credential exists.
pub(super) fn plan(
    request: &AlterUserScramCredentialsRequest,
    refused: impl Fn(&str) -> Option<Failure>,
    stored: impl Fn(Kind, &str) -> bool,
) -> Plan {
    let mut deletions: Vec<(&str, Kind)> = Vec::new();
    let mut upsertions: Vec<(&str, Kind, &ScramCredentialUpsertion)> = Vec::new();
    let mut failures: Vec<(String, Failure)> = Vec::new();
    // The key of all three is the user's name, not the name and the mechanism: a user touched twice in any
    // combination loses both operations (:188-226), and once a user has a failure every later operation on the
    // name is ignored (:189, :209).
    let failed =
        |failures: &[(String, Failure)], name: &str| failures.iter().any(|(n, _)| n == name);
    for d in &request.deletions {
        let name = d.name.as_str();
        if failed(&failures, name) {
            continue;
        }
        if let Some(at) = deletions.iter().position(|(n, _)| *n == name) {
            deletions.remove(at);
            failures.push((name.to_owned(), TWICE));
            continue;
        }
        match check_deletion(name, d.mechanism, &refused, &stored) {
            Ok(kind) => deletions.push((name, kind)),
            Err(why) => failures.push((name.to_owned(), why)),
        }
    }
    for u in &request.upsertions {
        let name = u.name.as_str();
        if failed(&failures, name) {
            continue;
        }
        let was_deleted = deletions.iter().position(|(n, _)| *n == name);
        let was_set = upsertions.iter().position(|(n, _, _)| *n == name);
        if was_deleted.is_some() || was_set.is_some() {
            if let Some(at) = was_deleted {
                deletions.remove(at);
            } else if let Some(at) = was_set {
                upsertions.remove(at);
            }
            failures.push((name.to_owned(), TWICE));
            continue;
        }
        match check_upsertion(name, u, &refused) {
            Ok(kind) => upsertions.push((name, kind, u)),
            Err(why) => failures.push((name.to_owned(), why)),
        }
    }
    let mut made = Vec::new();
    for (name, kind, u) in upsertions {
        match finish(kind, u) {
            Some(verifier) => made.push((name.to_owned(), verifier)),
            None => failures.push((name.to_owned(), EMPTY_KEY)),
        }
    }
    Plan {
        deletions: deletions
            .into_iter()
            .map(|(n, k)| (n.to_owned(), k))
            .collect(),
        upsertions: made,
        failures,
    }
}

/// Java's `SecretKeySpec` throws `IllegalArgumentException("Empty key")` for a salted password of no bytes; the
/// controller catches it (`ScramControlManager.java:278`) and `ApiError.fromThrowable` makes UNKNOWN_SERVER_ERROR of it,
/// with no message (`ApiError.java:35-41`).
const EMPTY_KEY: Failure = Failure {
    code: ResponseError::UnknownServerError.code(),
    message: None,
};

/// `validateScramUsernameAndMechanism` (:311-322): the name first, then the mechanism.
fn check_name_and_mechanism(name: &str, mechanism: i8) -> Result<Kind, Failure> {
    if name.is_empty() {
        return Err(failure(
            ResponseError::UnacceptableCredential,
            "Username must not be empty",
        ));
    }
    kind_of(mechanism).ok_or(failure(
        ResponseError::UnsupportedSaslMechanism,
        "Unknown SCRAM mechanism",
    ))
}

/// `validateDeletion` (:298-309), with the refusal of a configured user between the two checks.
fn check_deletion(
    name: &str,
    mechanism: i8,
    refused: &impl Fn(&str) -> Option<Failure>,
    stored: &impl Fn(Kind, &str) -> bool,
) -> Result<Kind, Failure> {
    let kind = check_name_and_mechanism(name, mechanism)?;
    if let Some(why) = refused(name) {
        return Err(why);
    }
    if !stored(kind, name) {
        return Err(failure(
            ResponseError::ResourceNotFound,
            "Attempt to delete a user credential that does not exist",
        ));
    }
    Ok(kind)
}

/// `validateUpsertion` (:284-296): the name, the mechanism, then the iteration count.
fn check_upsertion(
    name: &str,
    u: &ScramCredentialUpsertion,
    refused: &impl Fn(&str) -> Option<Failure>,
) -> Result<Kind, Failure> {
    let kind = check_name_and_mechanism(name, u.mechanism)?;
    if let Some(why) = refused(name) {
        return Err(why);
    }
    if u.iterations < MIN_ITERATIONS {
        return Err(failure(
            ResponseError::UnacceptableCredential,
            "Too few iterations",
        ));
    }
    if u.iterations > MAX_ITERATIONS {
        return Err(failure(
            ResponseError::UnacceptableCredential,
            "Too many iterations",
        ));
    }
    Ok(kind)
}

/// `finishUpsertion` (:260-282): the keys, from the salted password the client computed.
fn finish(kind: Kind, u: &ScramCredentialUpsertion) -> Option<Verifier> {
    if u.salted_password.is_empty() {
        return None;
    }
    let iterations = NonZeroU32::new(u32::try_from(u.iterations).ok()?)?;
    Some(Verifier::from_salted_password(
        kind,
        &u.salt,
        iterations,
        &u.salted_password,
    ))
}
