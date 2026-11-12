//! DeleteAcls: remove every ACL a filter selects.

use std::collections::BTreeSet;

use kafka_auth::authz::{Acl, AclFilter, AclSet, CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::delete_acls_request::DeleteAclsFilter;
use kafka_wire::messages::delete_acls_response::{DeleteAclsFilterResult, DeleteAclsMatchingAcl};
use kafka_wire::messages::{DeleteAclsRequest, DeleteAclsResponse};
use tracing::error;

use crate::authorizer::record_of;
use crate::{Broker, Caller, Outcome, blocking};

/// The most ACLs one request may remove (Kafka's `MAX_RECORDS_PER_USER_OP`).
const MOST_REMOVED: usize = 10_000;

fn filter_of(f: &DeleteAclsFilter) -> AclFilter {
    AclFilter {
        resource_type: f.resource_type_filter,
        name: f.resource_name_filter.as_ref().map(ToString::to_string),
        pattern_type: f.pattern_type_filter,
        principal: f.principal_filter.as_ref().map(ToString::to_string),
        host: f.host_filter.as_ref().map(ToString::to_string),
        operation: f.operation,
        permission: f.permission_type,
    }
}

fn failed(code: ResponseError, message: Option<&str>) -> DeleteAclsFilterResult {
    let mut r = DeleteAclsFilterResult::default();
    r.error_code = code.code();
    r.error_message = message.map(|m| StrBytes::from_string(m.to_owned()));
    r
}

fn matching(a: &Acl) -> DeleteAclsMatchingAcl {
    let text = |s: &str| StrBytes::from_string(s.to_owned());
    let mut m = DeleteAclsMatchingAcl::default();
    m.error_message = None;
    m.resource_type = a.resource_type;
    m.resource_name = text(&a.name);
    m.pattern_type = a.pattern_type;
    m.principal = text(&a.principal);
    m.host = text(&a.host);
    m.operation = a.operation;
    m.permission_type = a.permission;
    m
}

impl<F: Fs> Broker<F> {
    /// Removes the ACLs the filters select, all in one write, and lists for each filter what it selected.
    ///
    /// A code of 0 in any filter closes the connection, before anything is authorized. Then the caller needs ALTER
    /// on the cluster (every result 31). Each filter is judged against the ACLs as they were before the request, so
    /// two filters that select the same ACL both list it, and it is removed once. A filter with a code that is no
    /// value of its enumeration is answered 42 (`Unknown patternFilter.` or `Unknown entryFilter.`); one that
    /// selects nothing succeeds with an empty list and no message.
    pub(crate) async fn delete_acls(&self, caller: &Caller, req: &DeleteAclsRequest) -> Outcome {
        let filters: Vec<AclFilter> = req.filters.iter().map(filter_of).collect();
        if filters.iter().any(AclFilter::has_zero_code) {
            return Outcome::Close;
        }
        let _one_change_at_a_time = self.acl_admin.lock().await;
        let results =
            if self
                .authz
                .allows(caller, resource::CLUSTER, CLUSTER_NAME, operation::ALTER)
            {
                self.remove_acls(&filters)
            } else {
                filters
                    .iter()
                    .map(|_| failed(ResponseError::ClusterAuthorizationFailed, None))
                    .collect()
            };
        let mut resp = DeleteAclsResponse::default();
        resp.filter_results = results;
        Outcome::Respond(Response::DeleteAcls(resp))
    }

    fn remove_acls(&self, filters: &[AclFilter]) -> Vec<DeleteAclsFilterResult> {
        let snapshot = self.authz.snapshot();
        let mut doomed: BTreeSet<&Acl> = BTreeSet::new();
        let mut results = Vec::with_capacity(filters.len());
        for filter in filters {
            if let Err(why) = filter.validate() {
                results.push(failed(
                    ResponseError::InvalidRequest,
                    Some(&why.to_string()),
                ));
                continue;
            }
            let selected = snapshot.select(filter);
            doomed.extend(selected.iter().copied());
            let mut r = DeleteAclsFilterResult::default();
            // Success carries no message (the default is an empty one).
            r.error_message = None;
            r.matching_acls = selected.into_iter().map(matching).collect();
            results.push(r);
        }
        if doomed.len() > MOST_REMOVED {
            let why = format!(
                "Cannot remove more than {MOST_REMOVED} acls in a single delete operation."
            );
            return filters
                .iter()
                .map(|_| failed(ResponseError::InvalidRequest, Some(&why)))
                .collect();
        }
        if doomed.is_empty() {
            return results;
        }
        let records: Vec<_> = doomed.iter().map(|a| record_of(a)).collect();
        match blocking::run(|| self.meta.remove_acls(&records)) {
            Ok(()) => {
                let kept = snapshot.iter().filter(|a| !doomed.contains(a)).cloned();
                self.authz.replace(AclSet::new(kept));
                results
            }
            Err(e) => {
                error!(error = %e, "removing ACLs failed");
                results
                    .into_iter()
                    .map(|r| {
                        if r.error_code == 0 {
                            failed(ResponseError::KafkaStorageError, None)
                        } else {
                            r
                        }
                    })
                    .collect()
            }
        }
    }
}
