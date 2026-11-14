//! DescribeCluster v0 to v2 (WIRE-ish rules W24 to W26; SEC-290): the one broker of the cluster, as Metadata says it, and the
//! cluster's authorized operations, which are the only thing an ACL changes. The rows of the recorded `version-matrix`
//! (default request at each version) and the endpoint-type refusals of `AuthHelper.computeDescribeClusterResponse`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{DescribeClusterRequest, DescribeClusterResponse};
use support::acls::{
    ALL, ALLOW, ALTER, CLUSTER, CREATE, DENY, DESCRIBE, LITERAL, creation, grant, user,
};
use support::{Rig, rig, rig_with};

const fn bit(op: i8) -> i32 {
    1 << op
}

/// CREATE, ALTER, DESCRIBE, CLUSTER_ACTION, DESCRIBE_CONFIGS, ALTER_CONFIGS, IDEMPOTENT_WRITE.
const ALL_CLUSTER: i32 = bit(5) | bit(7) | bit(8) | bit(9) | bit(10) | bit(11) | bit(12);
const NOT_ASKED: i32 = i32::MIN;
const BROKER: i8 = 1;
const CONTROLLER: i8 = 2;

fn request(endpoint: i8, operations: bool) -> DescribeClusterRequest {
    let mut r = DescribeClusterRequest::default();
    r.endpoint_type = endpoint;
    r.include_cluster_authorized_operations = operations;
    r
}

async fn describe(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: DescribeClusterRequest,
) -> DescribeClusterResponse {
    let Response::DescribeCluster(r) = rig
        .ask_as(who, version, Request::DescribeCluster(req))
        .await
    else {
        panic!("not DescribeCluster")
    };
    r
}

fn cluster_acl(
    principal: &str,
    op: i8,
    permission: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation(
        (CLUSTER, "kafka-cluster", LITERAL),
        principal,
        op,
        permission,
    )
}

#[tokio::test]
async fn the_default_request_is_answered_as_the_recorded_kafka_does_at_every_version() {
    let rig = rig();
    let anyone = support::caller();
    for version in 0..=2 {
        let r = describe(&rig, &anyone, version, request(BROKER, false)).await;
        assert_eq!(
            (r.error_code, r.error_message.as_deref()),
            (0, None),
            "v{version}"
        );
        assert_eq!(&*r.cluster_id, "doze-kafka", "v{version}");
        assert_eq!(
            (r.controller_id.0, r.endpoint_type),
            (1, BROKER),
            "v{version}"
        );
        assert_eq!(r.cluster_authorized_operations, NOT_ASKED, "v{version}");
        assert_eq!(r.throttle_time_ms, 0);
        let [b] = &r.brokers[..] else {
            panic!("one broker: {:?}", r.brokers)
        };
        assert_eq!((b.broker_id.0, &*b.host, b.port), (1, "127.0.0.1", 4242));
        assert!(b.rack.is_none() && !b.is_fenced, "v{version}");
    }
}

#[tokio::test]
async fn it_names_the_cluster_and_the_broker_that_metadata_does() {
    let rig = rig();
    let anyone = support::caller();
    let m = rig.metadata_as(&anyone, 12, Some(&[]), false).await;
    let r = describe(&rig, &anyone, 2, request(BROKER, false)).await;
    assert_eq!(m.cluster_id.as_deref(), Some(&*r.cluster_id));
    assert_eq!(m.controller_id, r.controller_id);
    assert_eq!(
        (m.brokers[0].node_id, &*m.brokers[0].host, m.brokers[0].port),
        (
            r.brokers[0].broker_id,
            &*r.brokers[0].host,
            r.brokers[0].port
        )
    );
}

#[tokio::test]
async fn fenced_brokers_are_never_listed_because_this_one_is_never_fenced() {
    let rig = rig();
    let mut asked = request(BROKER, false);
    asked.include_fenced_brokers = true;
    let r = describe(&rig, &support::caller(), 2, asked).await;
    assert_eq!(r.brokers.len(), 1);
    assert!(!r.brokers[0].is_fenced);
}

#[tokio::test]
async fn a_controller_endpoint_is_asked_of_the_wrong_node() {
    let rig = rig();
    let anyone = support::caller();
    let said = "The request was sent to an endpoint of type BROKER, but we wanted an endpoint of type CONTROLLER";
    for (version, code) in [(0, 42), (1, 114), (2, 114)] {
        let r = describe(&rig, &anyone, version, request(CONTROLLER, true)).await;
        assert_eq!(r.error_code, code, "v{version}");
        assert_eq!(r.error_message.as_deref(), Some(said), "v{version}");
        assert!(
            r.brokers.is_empty() && r.cluster_id.is_empty(),
            "v{version}: no cluster is described"
        );
        assert_eq!(r.cluster_authorized_operations, NOT_ASKED, "v{version}");
    }
}

#[tokio::test]
async fn an_endpoint_type_that_is_neither_is_unsupported_and_says_which() {
    let rig = rig();
    for kind in [0, 3, -1, 127] {
        let r = describe(&rig, &support::caller(), 1, request(kind, false)).await;
        assert_eq!(r.error_code, 115, "type {kind}");
        assert_eq!(
            r.error_message.as_deref(),
            Some(format!("Unsupported endpoint type {kind}").as_str())
        );
        assert!(r.brokers.is_empty());
    }
}

#[tokio::test]
async fn no_acl_is_needed_to_be_answered_and_the_operations_are_what_describe_gates() {
    let rig = rig_with(support::acls::closed_world);
    grant(
        &rig,
        vec![
            cluster_acl("User:viewer", DESCRIBE, ALLOW),
            cluster_acl("User:viewer", CREATE, ALLOW),
            cluster_acl("User:owner", ALL, ALLOW),
            cluster_acl("User:altering", ALTER, ALLOW),
            cluster_acl("User:creator", CREATE, ALLOW),
            cluster_acl("User:barred", ALL, ALLOW),
            cluster_acl("User:barred", DESCRIBE, DENY),
        ],
    )
    .await;
    for version in 0..=2 {
        let ops = |who: &'static str, asked: bool| {
            let (rig, who) = (&rig, user(who));
            async move {
                let r = describe(rig, &who, version, request(BROKER, asked)).await;
                // Whoever asks is answered: the cluster is not a secret from a user with no rights at all.
                assert_eq!(
                    (r.error_code, r.brokers.len()),
                    (0, 1),
                    "{who:?} v{version}"
                );
                r.cluster_authorized_operations
            }
        };
        assert_eq!(ops("viewer", true).await, bit(8) | bit(5), "v{version}");
        assert_eq!(ops("owner", true).await, ALL_CLUSTER, "v{version}");
        assert_eq!(ops("altering", true).await, bit(7) | bit(8), "v{version}");
        // Without DESCRIBE on the cluster nothing is told, not even the right a user does hold; a DENY wins over ALL.
        for refused in ["creator", "barred", "stranger"] {
            assert_eq!(ops(refused, true).await, 0, "{refused} v{version}");
        }
        assert_eq!(
            ops("admin", true).await,
            ALL_CLUSTER,
            "a super user, v{version}"
        );
        // Not asked for: the schema's default, whoever asks.
        for who in ["viewer", "stranger", "admin"] {
            assert_eq!(ops(who, false).await, NOT_ASKED, "{who} v{version}");
        }
    }
}

#[tokio::test]
async fn an_open_broker_tells_every_operation() {
    let rig = rig();
    let r = describe(&rig, &support::caller(), 2, request(BROKER, true)).await;
    assert_eq!(r.cluster_authorized_operations, ALL_CLUSTER);
}
