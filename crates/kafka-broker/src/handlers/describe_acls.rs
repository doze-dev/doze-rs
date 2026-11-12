//! DescribeAcls: the ACLs a filter selects.

use kafka_auth::authz::{Acl, AclFilter, CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::DescribeAclsRequest;
use kafka_wire::messages::describe_acls_response::{AclDescription, DescribeAclsResource};

use crate::{Broker, Caller, Outcome};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

pub(super) fn filter_of(r: &DescribeAclsRequest) -> AclFilter {
    AclFilter {
        resource_type: r.resource_type_filter,
        name: r.resource_name_filter.as_ref().map(ToString::to_string),
        pattern_type: r.pattern_type_filter,
        principal: r.principal_filter.as_ref().map(ToString::to_string),
        host: r.host_filter.as_ref().map(ToString::to_string),
        operation: r.operation,
        permission: r.permission_type,
    }
}

impl<F: Fs> Broker<F> {
    /// Answers with the ACLs the filter selects, grouped by resource and sorted (Kafka's order is a hash map's; a
    /// client sorts). A code of 0 in the filter closes the connection, before anything is authorized. A filter with a
    /// code that is no value of its enumeration selects nothing. The error message is the empty string, as Kafka sends.
    pub(crate) fn describe_acls(&self, caller: &Caller, req: &DescribeAclsRequest) -> Outcome {
        let filter = filter_of(req);
        if filter.has_zero_code() {
            return Outcome::Close;
        }
        let mut resp = kafka_wire::messages::DescribeAclsResponse::default();
        resp.error_message = Some(text(""));
        if !self
            .authz
            .allows(caller, resource::CLUSTER, CLUSTER_NAME, operation::DESCRIBE)
        {
            resp.error_code = ResponseError::ClusterAuthorizationFailed.code();
            return Outcome::Respond(Response::DescribeAcls(resp));
        }
        if filter.validate().is_ok() {
            let snapshot = self.authz.snapshot();
            let mut selected = snapshot.select(&filter).into_iter().peekable();
            // Sorted by (type, name, pattern) first, so a resource's ACLs are next to each other.
            while let Some(first) = selected.next() {
                let mut group = DescribeAclsResource::default();
                group.resource_type = first.resource_type;
                group.resource_name = text(&first.name);
                group.pattern_type = first.pattern_type;
                group.acls.push(entry(first));
                while let Some(next) = selected.next_if(|a| same_resource(a, first)) {
                    group.acls.push(entry(next));
                }
                resp.resources.push(group);
            }
        }
        Outcome::Respond(Response::DescribeAcls(resp))
    }
}

fn same_resource(a: &Acl, b: &Acl) -> bool {
    (a.resource_type, &a.name, a.pattern_type) == (b.resource_type, &b.name, b.pattern_type)
}

fn entry(a: &Acl) -> AclDescription {
    let mut d = AclDescription::default();
    d.principal = text(&a.principal);
    d.host = text(&a.host);
    d.operation = a.operation;
    d.permission_type = a.permission;
    d
}
