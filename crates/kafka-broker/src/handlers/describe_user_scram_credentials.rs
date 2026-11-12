//! DescribeUserScramCredentials (API 50): which users have SCRAM credentials, for which mechanisms and how many
//! iterations. Never the salt or a key.

use kafka_auth::authz::{CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::describe_user_scram_credentials_response::{
    CredentialInfo, DescribeUserScramCredentialsResult,
};
use kafka_wire::messages::{
    DescribeUserScramCredentialsRequest, DescribeUserScramCredentialsResponse,
};

use crate::users::number_of;
use crate::{Broker, Caller, Outcome};

const DUPLICATE: &str =
    "Cannot describe SCRAM credentials for the same user twice in a single request: ";
const MISSING: &str = "Attempt to describe a user credential that does not exist: ";

fn result(user: &str, code: i16, message: Option<String>) -> DescribeUserScramCredentialsResult {
    let mut r = DescribeUserScramCredentialsResult::default();
    r.user = StrBytes::from_string(user.to_owned());
    r.error_code = code;
    r.error_message = message.map(StrBytes::from_string);
    r
}

impl<F: Fs> Broker<F> {
    /// Answers for the users named, or for every user that has a credential if none is named (a missing list and an
    /// empty one mean the same: `ScramImage.java:84-89`). A name given more than once is one entry, DUPLICATE_RESOURCE,
    /// however many times and whether or not it exists; a name with no credential is RESOURCE_NOT_FOUND
    /// (`ScramImage.java:96-127`). Kafka's order is a hash map's; here it is the order of first appearance, or by name.
    ///
    /// The caller needs DESCRIBE on the cluster. Without it the whole request is 31 and so is one entry for each name
    /// asked for (`DescribeUserScramCredentialsRequest.getErrorResponse`), with no message (SEC-D8).
    pub(crate) fn describe_user_scram_credentials(
        &self,
        caller: &Caller,
        req: &DescribeUserScramCredentialsRequest,
    ) -> Outcome {
        let mut resp = DescribeUserScramCredentialsResponse::default();
        let allowed =
            self.authz
                .allows(caller, resource::CLUSTER, CLUSTER_NAME, operation::DESCRIBE);
        if !allowed {
            let code = ResponseError::ClusterAuthorizationFailed;
            resp.error_code = code.code();
            resp.error_message = None;
            resp.results = (req.users.iter().flatten())
                .map(|_| result("", code.code(), None))
                .collect();
            return Outcome::Respond(Response::DescribeUserScramCredentials(resp));
        }
        resp.results = match req.users.as_deref() {
            None | Some([]) => (self.users.scram_credentials().into_iter())
                .map(|(user, kinds)| found(&user, &kinds))
                .collect(),
            Some(asked) => {
                let mut seen: Vec<(&str, bool)> = Vec::new();
                for name in asked.iter().map(|u| u.name.as_str()) {
                    match seen.iter_mut().find(|(n, _)| *n == name) {
                        Some(entry) => entry.1 = true,
                        None => seen.push((name, false)),
                    }
                }
                seen.into_iter()
                    .map(|(name, twice)| self.one(name, twice))
                    .collect()
            }
        };
        Outcome::Respond(Response::DescribeUserScramCredentials(resp))
    }

    fn one(&self, name: &str, repeated: bool) -> DescribeUserScramCredentialsResult {
        if repeated {
            return result(
                name,
                ResponseError::DuplicateResource.code(),
                Some(format!("{DUPLICATE}{name}")),
            );
        }
        let kinds = self.users.scram_of(name);
        if kinds.is_empty() {
            return result(
                name,
                ResponseError::ResourceNotFound.code(),
                Some(format!("{MISSING}{name}")),
            );
        }
        found(name, &kinds)
    }
}

fn found(
    user: &str,
    kinds: &[(kafka_auth::scram::Kind, u32)],
) -> DescribeUserScramCredentialsResult {
    // As Kafka's generated class leaves them: no error and the empty message (""), not null.
    let mut r = DescribeUserScramCredentialsResult::default();
    r.user = StrBytes::from_string(user.to_owned());
    r.credential_infos = kinds
        .iter()
        .map(|&(kind, iterations)| {
            let mut info = CredentialInfo::default();
            info.mechanism = number_of(kind);
            info.iterations = i32::try_from(iterations).unwrap_or(i32::MAX);
            info
        })
        .collect();
    r
}
