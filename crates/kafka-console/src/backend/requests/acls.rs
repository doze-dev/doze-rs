//! The requests of the Access page: every ACL the broker has, one created, one deleted by its exact fields, and the
//! names of the users that have a SCRAM credential (never the credential: the request asks for names, and the answer
//! is read for names and mechanism numbers only).

use kafka_auth::authz::{pattern, permission};
use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::delete_acls_request::DeleteAclsFilter;
use kafka_wire::messages::{
    CreateAclsRequest, DeleteAclsRequest, DescribeAclsRequest, DescribeUserScramCredentialsRequest,
};

use crate::model::{AclEntry, AclPattern, AclPermission};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn pattern_of(p: AclPattern) -> i8 {
    match p {
        AclPattern::Literal => pattern::LITERAL,
        AclPattern::Prefixed => pattern::PREFIXED,
    }
}

fn permission_of(p: AclPermission) -> i8 {
    match p {
        AclPermission::Allow => permission::ALLOW,
        AclPermission::Deny => permission::DENY,
    }
}

/// Every ACL: a filter that names nothing and so matches all (ANY for the four codes, null for the three texts).
#[must_use]
pub fn describe_all() -> Request {
    let mut r = DescribeAclsRequest::default();
    r.resource_type_filter = 1;
    r.resource_name_filter = None;
    r.pattern_type_filter = pattern::ANY;
    r.principal_filter = None;
    r.host_filter = None;
    r.operation = 1;
    r.permission_type = permission::ANY;
    Request::DescribeAcls(r)
}

/// Creates this one ACL, as written.
#[must_use]
pub fn create(e: &AclEntry) -> Request {
    let mut c = AclCreation::default();
    c.resource_type = e.kind;
    c.resource_name = text(&e.name);
    c.resource_pattern_type = pattern_of(e.pattern);
    c.principal = text(&e.principal);
    c.host = text(&e.host);
    c.operation = e.operation;
    c.permission_type = permission_of(e.permission);
    let mut r = CreateAclsRequest::default();
    r.creations = vec![c];
    Request::CreateAcls(r)
}

/// Deletes exactly this ACL: one filter with every field named, so no other ACL can match it.
#[must_use]
pub fn delete_exact(e: &AclEntry) -> Request {
    let mut f = DeleteAclsFilter::default();
    f.resource_type_filter = e.kind;
    f.resource_name_filter = Some(text(&e.name));
    f.pattern_type_filter = pattern_of(e.pattern);
    f.principal_filter = Some(text(&e.principal));
    f.host_filter = Some(text(&e.host));
    f.operation = e.operation;
    f.permission_type = permission_of(e.permission);
    let mut r = DeleteAclsRequest::default();
    r.filters = vec![f];
    Request::DeleteAcls(r)
}

/// Every user with a SCRAM credential (no names means all).
#[must_use]
pub fn describe_users() -> Request {
    let mut r = DescribeUserScramCredentialsRequest::default();
    r.users = None;
    Request::DescribeUserScramCredentials(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> AclEntry {
        AclEntry {
            kind: 2,
            name: "orders".into(),
            pattern: AclPattern::Prefixed,
            principal: "User:orders".into(),
            host: "*".into(),
            operation: 4,
            permission: AclPermission::Deny,
        }
    }

    #[test]
    fn describe_names_nothing_so_it_matches_every_acl() {
        let Request::DescribeAcls(r) = describe_all() else {
            panic!("not DescribeAcls")
        };
        assert_eq!((r.resource_type_filter, r.pattern_type_filter), (1, 1));
        assert_eq!((r.operation, r.permission_type), (1, 1));
        assert!(r.resource_name_filter.is_none() && r.principal_filter.is_none());
        assert!(r.host_filter.is_none());
    }

    #[test]
    fn a_creation_and_an_exact_delete_carry_every_field_as_typed() {
        let Request::CreateAcls(c) = create(&entry()) else {
            panic!("not CreateAcls")
        };
        let a = &c.creations[0];
        assert_eq!(
            (
                a.resource_type,
                a.resource_pattern_type,
                a.operation,
                a.permission_type
            ),
            (2, 4, 4, 2)
        );
        assert_eq!(
            (
                a.resource_name.as_str(),
                a.principal.as_str(),
                a.host.as_str()
            ),
            ("orders", "User:orders", "*")
        );
        let Request::DeleteAcls(d) = delete_exact(&entry()) else {
            panic!("not DeleteAcls")
        };
        let f = &d.filters[0];
        assert_eq!(d.filters.len(), 1);
        assert_eq!(
            (
                f.resource_type_filter,
                f.pattern_type_filter,
                f.operation,
                f.permission_type
            ),
            (2, 4, 4, 2)
        );
        assert_eq!(
            f.resource_name_filter.as_ref().map(|s| s.as_str()),
            Some("orders")
        );
        assert_eq!(
            f.principal_filter.as_ref().map(|s| s.as_str()),
            Some("User:orders")
        );
        assert_eq!(f.host_filter.as_ref().map(|s| s.as_str()), Some("*"));
    }

    #[test]
    fn the_user_list_asks_for_everybody() {
        let Request::DescribeUserScramCredentials(r) = describe_users() else {
            panic!("not DescribeUserScramCredentials")
        };
        assert!(r.users.is_none());
    }
}
