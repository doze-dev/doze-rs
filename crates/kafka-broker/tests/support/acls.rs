//! Requests for the ACL APIs, and callers to make them as.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::{Rig, at, head};
use kafka_auth::Principal;
use kafka_broker::{Caller, Outcome};
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::create_acls_response::AclCreationResult;
use kafka_wire::messages::delete_acls_request::DeleteAclsFilter;
use kafka_wire::messages::delete_acls_response::DeleteAclsFilterResult;
use kafka_wire::messages::{
    CreateAclsRequest, DeleteAclsRequest, DescribeAclsRequest, DescribeAclsResponse,
};

/// An ACL as DescribeAcls lists it: type, name, pattern, principal, host, operation, permission.
pub type Entry = (i8, String, i8, String, String, i8, i8);

pub const TOPIC: i8 = 2;
pub const GROUP: i8 = 3;
pub const CLUSTER: i8 = 4;
pub const TXN_ID: i8 = 5;
pub const LITERAL: i8 = 3;
pub const PREFIXED: i8 = 4;
pub const READ: i8 = 3;
pub const WRITE: i8 = 4;
pub const CREATE: i8 = 5;
pub const ALTER: i8 = 7;
pub const DESCRIBE: i8 = 8;
pub const IDEMPOTENT_WRITE: i8 = 12;
pub const ALL: i8 = 2;
pub const DENY: i8 = 2;
pub const ALLOW: i8 = 3;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A broker whose default refuses (`allow.everyone.if.no.acl.found` false) and whose super user is `admin`.
pub fn closed_world(c: &mut kafka_broker::Config) {
    c.acl_default_allow = false;
    c.super_users.insert("User:admin".to_owned());
}

/// The super user of `closed_world`.
pub fn admin() -> Caller {
    user("admin")
}

/// `name` logged in, connected from 10.0.0.1.
pub fn user(name: &str) -> Caller {
    Caller::new(Principal::user(name), Some("10.0.0.1".parse().unwrap()))
}

/// A creation for any principal string, from any host (`*`).
pub fn creation(
    (res, name, pat): (i8, &str, i8),
    principal: &str,
    op: i8,
    perm: i8,
) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = res;
    c.resource_name = text(name);
    c.resource_pattern_type = pat;
    c.principal = text(principal);
    c.host = text("*");
    c.operation = op;
    c.permission_type = perm;
    c
}

/// A filter of `(res, pat, op, perm)` with no name, principal or host.
pub fn filter(res: i8, pat: i8, op: i8, perm: i8) -> DeleteAclsFilter {
    let mut f = DeleteAclsFilter::default();
    // The crate's default for a nullable string is the empty one; a filter that selects everything has nulls.
    (f.resource_name_filter, f.principal_filter, f.host_filter) = (None, None, None);
    f.resource_type_filter = res;
    f.pattern_type_filter = pat;
    f.operation = op;
    f.permission_type = perm;
    f
}

pub fn describe_request(f: &DeleteAclsFilter) -> DescribeAclsRequest {
    let mut r = DescribeAclsRequest::default();
    r.resource_type_filter = f.resource_type_filter;
    r.resource_name_filter = f.resource_name_filter.clone();
    r.pattern_type_filter = f.pattern_type_filter;
    r.principal_filter = f.principal_filter.clone();
    r.host_filter = f.host_filter.clone();
    r.operation = f.operation;
    r.permission_type = f.permission_type;
    r
}

impl Rig {
    /// [`Rig::create`] as the super user of `closed_world` (the anonymous caller may not create topics there).
    pub async fn create_as_admin(
        &self,
        topics: Vec<kafka_wire::messages::create_topics_request::CreatableTopic>,
        validate_only: bool,
    ) -> Vec<kafka_wire::messages::create_topics_response::CreatableTopicResult> {
        let mut req = kafka_wire::messages::CreateTopicsRequest::default();
        req.topics = topics;
        req.validate_only = validate_only;
        let Response::CreateTopics(r) = self.ask_as(&admin(), 7, Request::CreateTopics(req)).await
        else {
            panic!("not CreateTopics")
        };
        r.topics
    }

    /// What the broker does with `request`, from `caller`.
    pub async fn outcome_as(&self, caller: &Caller, version: i16, request: Request) -> Outcome {
        self.broker
            .handle(&at(), caller, &head(request.api_key(), version), request)
            .await
    }

    pub async fn create_acls_as(
        &self,
        who: &Caller,
        version: i16,
        creations: Vec<AclCreation>,
    ) -> Vec<AclCreationResult> {
        let mut req = CreateAclsRequest::default();
        req.creations = creations;
        let Response::CreateAcls(r) = self.ask_as(who, version, Request::CreateAcls(req)).await
        else {
            panic!("not CreateAcls")
        };
        r.results
    }

    pub async fn describe_acls_as(
        &self,
        who: &Caller,
        version: i16,
        filter: &DeleteAclsFilter,
    ) -> DescribeAclsResponse {
        let req = Request::DescribeAcls(describe_request(filter));
        let Response::DescribeAcls(r) = self.ask_as(who, version, req).await else {
            panic!("not DescribeAcls")
        };
        r
    }

    pub async fn delete_acls_as(
        &self,
        who: &Caller,
        version: i16,
        filters: Vec<DeleteAclsFilter>,
    ) -> Vec<DeleteAclsFilterResult> {
        let mut req = DeleteAclsRequest::default();
        req.filters = filters;
        let Response::DeleteAcls(r) = self.ask_as(who, version, Request::DeleteAcls(req)).await
        else {
            panic!("not DeleteAcls")
        };
        r.filter_results
    }

    /// Metadata for these topics (None: all), at `version`, as `caller`.
    pub async fn metadata_as(
        &self,
        caller: &Caller,
        version: i16,
        names: Option<&[&str]>,
        allow_auto: bool,
    ) -> kafka_wire::messages::MetadataResponse {
        let mut req = kafka_wire::messages::MetadataRequest::default();
        req.allow_auto_topic_creation = allow_auto;
        req.topics = names.map(|ns| {
            ns.iter()
                .map(|n| {
                    let mut t =
                        kafka_wire::messages::metadata_request::MetadataRequestTopic::default();
                    t.name = Some(super::wire_name(n));
                    t
                })
                .collect()
        });
        let Response::Metadata(r) = self.ask_as(caller, version, Request::Metadata(req)).await
        else {
            panic!("not Metadata")
        };
        r
    }

    /// InitProducerId (v4) as `caller`: (error code, producer id).
    pub async fn init_producer_id_as(&self, caller: &Caller, txn_id: Option<&str>) -> (i16, i64) {
        let mut req = kafka_wire::messages::InitProducerIdRequest::default();
        req.transactional_id = txn_id.map(|t| kafka_wire::messages::TransactionalId(text(t)));
        req.transaction_timeout_ms = 60_000;
        let Response::InitProducerId(r) =
            self.ask_as(caller, 4, Request::InitProducerId(req)).await
        else {
            panic!("not InitProducerId")
        };
        (r.error_code, r.producer_id.0)
    }

    /// Every ACL, as `(type, name, pattern, principal, host, operation, permission)`, as DescribeAcls lists them
    /// for a caller that may.
    pub async fn acls_as(&self, who: &Caller) -> Vec<(i8, String, i8, String, String, i8, i8)> {
        let r = self.describe_acls_as(who, 3, &filter(1, 1, 1, 1)).await;
        assert_eq!(r.error_code, 0);
        let mut out = Vec::new();
        for res in &r.resources {
            for a in &res.acls {
                out.push((
                    res.resource_type,
                    res.resource_name.to_string(),
                    res.pattern_type,
                    a.principal.to_string(),
                    a.host.to_string(),
                    a.operation,
                    a.permission_type,
                ));
            }
        }
        out
    }
}

/// The topics there are, as the super user sees them.
pub async fn topics_of(rig: &Rig) -> Vec<String> {
    let mut names: Vec<_> = shown(&rig.metadata_as(&user("admin"), 12, None, false).await)
        .into_iter()
        .map(|t| t.0)
        .collect();
    names.sort();
    names
}

pub async fn grant(rig: &Rig, acls: Vec<kafka_wire::messages::create_acls_request::AclCreation>) {
    let r = rig.create_acls_as(&user("admin"), 3, acls).await;
    assert!(r.iter().all(|x| x.error_code == 0), "{r:?}");
}

pub fn on(
    name: &str,
    principal: &str,
    op: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((TOPIC, name, LITERAL), principal, op, ALLOW)
}

/// `(name, error code, partitions)` of each topic in the answer.
pub fn shown(r: &kafka_wire::messages::MetadataResponse) -> Vec<(String, i16, usize)> {
    r.topics
        .iter()
        .map(|t| {
            (
                t.name.as_ref().map_or(String::new(), |n| n.0.to_string()),
                t.error_code,
                t.partitions.len(),
            )
        })
        .collect()
}
