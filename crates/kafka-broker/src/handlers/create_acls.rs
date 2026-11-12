//! CreateAcls: store ACLs, and enforce them from the next request on.

use std::collections::{BTreeSet, HashSet};

use kafka_auth::authz::{Acl, AclSet, CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::create_acls_response::AclCreationResult;
use kafka_wire::messages::{CreateAclsRequest, CreateAclsResponse};
use tracing::error;

use crate::authorizer::record_of;
use crate::{Broker, Caller, Outcome, blocking};

fn acl_of(c: &AclCreation) -> Acl {
    Acl {
        resource_type: c.resource_type,
        name: c.resource_name.to_string(),
        pattern_type: c.resource_pattern_type,
        principal: c.principal.to_string(),
        host: c.host.to_string(),
        operation: c.operation,
        permission: c.permission_type,
    }
}

/// A result that says `code` and, if there is one, why.
fn result(code: i16, message: Option<&str>) -> AclCreationResult {
    let mut r = AclCreationResult::default();
    r.error_code = code;
    r.error_message = message.map(|m| StrBytes::from_string(m.to_owned()));
    r
}

impl<F: Fs> Broker<F> {
    /// Stores the ACLs of the request that are valid, all in one write, and answers each in the order asked.
    ///
    /// The order of the checks is Kafka's: a code of 0 anywhere closes the connection; then the caller needs ALTER
    /// on the cluster (every result 31); then a code only a filter can have (ANY, MATCH) fails every result with
    /// -1 and no message, as a request Kafka cannot even build; then each ACL is checked in turn and answered 42
    /// with Kafka's words, or 0 with an empty message. An ACL already stored, or twice in the request, is stored
    /// once and reports success.
    pub(crate) async fn create_acls(&self, caller: &Caller, req: &CreateAclsRequest) -> Outcome {
        let acls: Vec<Acl> = req.creations.iter().map(acl_of).collect();
        if acls.iter().any(Acl::has_zero_code) {
            return Outcome::Close;
        }
        let _one_change_at_a_time = self.acl_admin.lock().await;
        let all = |code: ResponseError, message: Option<&str>| {
            acls.iter().map(|_| result(code.code(), message)).collect()
        };
        let results: Vec<AclCreationResult> =
            if !self
                .authz
                .allows(caller, resource::CLUSTER, CLUSTER_NAME, operation::ALTER)
            {
                all(ResponseError::ClusterAuthorizationFailed, None)
            } else if acls.iter().any(Acl::has_filter_only_code) {
                all(ResponseError::UnknownServerError, None)
            } else {
                self.store_acls(&acls)
            };
        let mut resp = CreateAclsResponse::default();
        resp.results = results;
        Outcome::Respond(Response::CreateAcls(resp))
    }

    /// Checks each ACL and stores the valid ones; the answer for each, in order.
    fn store_acls(&self, acls: &[Acl]) -> Vec<AclCreationResult> {
        let refusals: Vec<Option<String>> = acls.iter().map(Acl::refusal).collect();
        let valid: BTreeSet<&Acl> = acls
            .iter()
            .zip(&refusals)
            .filter(|(_, why)| why.is_none())
            .map(|(a, _)| a)
            .collect();
        let snapshot = self.authz.snapshot();
        // Looked up in a set: a batch of N against M stored is N + M, not N times M, on a worker that other requests wait for.
        let stored_already: HashSet<&Acl> = snapshot.iter().collect();
        let fresh: Vec<&Acl> = valid
            .iter()
            .copied()
            .filter(|a| !stored_already.contains(*a))
            .collect();
        let stored = if fresh.is_empty() {
            Ok(())
        } else {
            let records: Vec<_> = fresh.iter().map(|a| record_of(a)).collect();
            blocking::run(|| self.meta.add_acls(&records))
        };
        match stored {
            Ok(()) => {
                if !fresh.is_empty() {
                    self.authz.replace(AclSet::new(
                        snapshot.iter().cloned().chain(fresh.into_iter().cloned()),
                    ));
                }
                refusals
                    .iter()
                    .map(|why| match why {
                        Some(message) => {
                            result(ResponseError::InvalidRequest.code(), Some(message))
                        }
                        None => result(0, Some("")),
                    })
                    .collect()
            }
            Err(e) => {
                error!(error = %e, "storing ACLs failed");
                refusals
                    .iter()
                    .map(|why| match why {
                        Some(message) => {
                            result(ResponseError::InvalidRequest.code(), Some(message))
                        }
                        None => result(ResponseError::KafkaStorageError.code(), None),
                    })
                    .collect()
            }
        }
    }
}
