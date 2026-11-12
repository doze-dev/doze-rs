//! The three ACL APIs, part one: the recorded answers (`acl-unknown-codes`) and the refusals of a creation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Outcome;
use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::{CreateAclsRequest, DeleteAclsRequest};
use support::acls::{
    ALLOW, ALTER, CLUSTER, DENY, LITERAL, PREFIXED, READ, TOPIC, admin, closed_world, creation,
    describe_request, filter,
};
use support::rig_with;

fn text(s: &str) -> Option<StrBytes> {
    Some(StrBytes::from_string(s.to_owned()))
}

/// A recorded step: its name and (resource, pattern, operation, permission).
type Step = (&'static str, (i8, i8, i8, i8));

/// The recorded `acl-unknown-codes` filter steps, as the wire carries them (res, pattern, operation, permission).
/// 0 is UNKNOWN and closes the connection; 99 and -1 are no code at all.
#[tokio::test]
async fn the_recorded_filter_steps_describe_and_delete() {
    let rig = rig_with(closed_world);
    let steps: [Step; 12] = [
        ("any", (1, 1, 1, 1)),
        ("topic-literal", (2, 3, 1, 1)),
        ("resource-0", (0, 3, 1, 1)),
        ("pattern-0", (2, 0, 1, 1)),
        ("operation-0", (2, 3, 0, 1)),
        ("permission-0", (2, 3, 1, 0)),
        ("all-0", (0, 0, 0, 0)),
        ("resource-99", (99, 3, 1, 1)),
        ("pattern-99", (2, 99, 1, 1)),
        ("operation-99", (2, 3, 99, 1)),
        ("permission-99", (2, 3, 1, 99)),
        ("resource-negative", (-1, 3, 1, 1)),
    ];
    for (tag, (res, pat, op, perm)) in steps {
        let f = filter(res, pat, op, perm);
        let describe = rig
            .outcome_as(&admin(), 3, Request::DescribeAcls(describe_request(&f)))
            .await;
        let mut req = DeleteAclsRequest::default();
        req.filters = vec![f];
        let delete = rig.outcome_as(&admin(), 3, Request::DeleteAcls(req)).await;
        if [res, pat, op, perm].contains(&0) {
            assert!(matches!(describe, Outcome::Close), "{tag}: describe closes");
            assert!(matches!(delete, Outcome::Close), "{tag}: delete closes");
            continue;
        }
        let kafka_wire::api::Response::DescribeAcls(d) = response(describe) else {
            panic!("{tag}")
        };
        assert_eq!(
            (d.error_code, d.error_message, d.resources.len()),
            (0, text(""), 0),
            "{tag}"
        );
        let kafka_wire::api::Response::DeleteAcls(r) = response(delete) else {
            panic!("{tag}")
        };
        let r = &r.filter_results[0];
        let unknown = match tag {
            "resource-99" | "pattern-99" | "resource-negative" => Some("Unknown patternFilter."),
            "operation-99" | "permission-99" => Some("Unknown entryFilter."),
            _ => None,
        };
        assert_eq!(
            (r.error_code, r.error_message.clone(), r.matching_acls.len()),
            (
                if unknown.is_some() { 42 } else { 0 },
                unknown.and_then(text),
                0
            ),
            "{tag}"
        );
    }
}

fn response(o: Outcome) -> kafka_wire::api::Response {
    match o {
        Outcome::Respond(r) => r,
        other => panic!("{other:?}"),
    }
}

/// The recorded `acl-unknown-codes` create step: (res, pattern, operation, permission) on `acl`, alice, `*`.
fn recorded(
    res: i8,
    pat: i8,
    op: i8,
    perm: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    let mut c = creation((2, "acl", 3), "User:alice", 3, 3);
    (
        c.resource_type,
        c.resource_pattern_type,
        c.operation,
        c.permission_type,
    ) = (res, pat, op, perm);
    c
}

/// `good` is stored with the empty message; a 0 in any field closes the connection and creates nothing.
#[tokio::test]
async fn the_recorded_create_steps_good_and_the_zeros() {
    let rig = rig_with(closed_world);
    let r = rig
        .create_acls_as(&admin(), 3, vec![recorded(2, 3, 3, 3)])
        .await;
    assert_eq!((r[0].error_code, r[0].error_message.clone()), (0, text("")));
    assert_eq!(rig.acls_as(&admin()).await.len(), 1);
    for (tag, c) in [
        ("resource-0", recorded(0, 3, 3, 3)),
        ("pattern-0", recorded(2, 0, 3, 3)),
        ("operation-0", recorded(2, 3, 0, 3)),
        ("permission-0", recorded(2, 3, 3, 0)),
    ] {
        let mut req = CreateAclsRequest::default();
        req.creations = vec![c];
        let o = rig.outcome_as(&admin(), 3, Request::CreateAcls(req)).await;
        assert!(matches!(o, Outcome::Close), "{tag}");
    }
    assert_eq!(
        rig.acls_as(&admin()).await.len(),
        1,
        "only `good` was stored"
    );
}

/// ANY and MATCH cannot be built into an ACL: -1 and no message. A number that is no code is 42 in Kafka's words.
#[tokio::test]
async fn the_recorded_create_steps_any_and_the_codes_that_are_none() {
    let rig = rig_with(closed_world);
    for (tag, c) in [
        ("resource-any", recorded(1, 3, 3, 3)),
        ("pattern-any", recorded(2, 1, 3, 3)),
        ("pattern-match", recorded(2, 2, 3, 3)),
        ("operation-any", recorded(2, 3, 1, 3)),
        ("permission-any", recorded(2, 3, 3, 1)),
    ] {
        let r = rig.create_acls_as(&admin(), 3, vec![c]).await;
        let got = (r[0].error_code, r[0].error_message.clone());
        assert_eq!(got, (-1, None), "{tag}");
    }
    for (tag, c, why) in [
        (
            "resource-99",
            recorded(99, 3, 3, 3),
            "Invalid resourceType UNKNOWN",
        ),
        (
            "pattern-99",
            recorded(2, 99, 3, 3),
            "Invalid patternType UNKNOWN",
        ),
        (
            "operation-99",
            recorded(2, 3, 99, 3),
            "Invalid operation UNKNOWN",
        ),
        (
            "permission-99",
            recorded(2, 3, 3, 99),
            "Invalid permissionType UNKNOWN",
        ),
    ] {
        let r = rig.create_acls_as(&admin(), 3, vec![c]).await;
        let got = (r[0].error_code, r[0].error_message.clone());
        assert_eq!(got, (42, text(why)), "{tag}");
    }
    assert!(
        rig.acls_as(&admin()).await.is_empty(),
        "none of them was stored"
    );
}

#[tokio::test]
async fn one_bad_item_in_a_request_decides_as_kafka_decides() {
    let rig = rig_with(closed_world);
    let good = |n: &str| creation((TOPIC, n, LITERAL), "User:alice", READ, ALLOW);
    let mut zero = good("z");
    zero.operation = 0;
    let mut any = good("a");
    any.permission_type = 1;
    let mut bad_code = good("b");
    bad_code.operation = 99;
    let mut req = CreateAclsRequest::default();
    req.creations = vec![good("1"), zero];
    let o = rig.outcome_as(&admin(), 3, Request::CreateAcls(req)).await;
    assert!(
        matches!(o, Outcome::Close),
        "a 0 anywhere closes the connection"
    );
    assert!(
        rig.acls_as(&admin()).await.is_empty(),
        "and nothing is stored"
    );
    // An ANY turns every result into -1, the good ones included, and nothing is stored.
    let r = rig.create_acls_as(&admin(), 3, vec![good("1"), any]).await;
    assert_eq!(r.iter().map(|x| x.error_code).collect::<Vec<_>>(), [-1, -1]);
    assert!(rig.acls_as(&admin()).await.is_empty());
    // A code that is no code fails its own item only; the others are stored, answered in the order asked.
    let r = rig
        .create_acls_as(&admin(), 3, vec![good("1"), bad_code, good("2")])
        .await;
    assert_eq!(
        r.iter().map(|x| x.error_code).collect::<Vec<_>>(),
        [0, 42, 0]
    );
    assert_eq!(rig.acls_as(&admin()).await.len(), 2);
}

#[tokio::test]
async fn the_other_refusals_of_a_creation_are_kafkas_words() {
    let rig = rig_with(closed_world);
    let mut empty_name = creation((TOPIC, "x", LITERAL), "User:a", READ, ALLOW);
    empty_name.resource_name = StrBytes::from_string(String::new());
    let r = rig
        .create_acls_as(
            &admin(),
            3,
            vec![
                empty_name,
                creation((CLUSTER, "my-cluster", LITERAL), "User:a", ALTER, ALLOW),
                creation((TOPIC, "x", LITERAL), "alice", READ, ALLOW),
                // Allowed, though odd: kept as sent.
                creation((CLUSTER, "kafka-cluster", LITERAL), "User:*", ALTER, DENY),
                creation((TOPIC, "*", PREFIXED), "User:", READ, ALLOW),
            ],
        )
        .await;
    let got: Vec<_> = r
        .iter()
        .map(|x| {
            (
                x.error_code,
                x.error_message.as_ref().map(ToString::to_string),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (42, Some("Invalid empty resource name".to_owned())),
            (42, Some("The only valid name for the CLUSTER resource is kafka-cluster".to_owned())),
            (42, Some("Could not parse principal from `alice` (no colon is present separating the principal type from the principal name)".to_owned())),
            (0, Some(String::new())),
            (0, Some(String::new())),
        ]
    );
}
