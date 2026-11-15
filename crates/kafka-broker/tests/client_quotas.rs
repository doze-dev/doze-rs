//! DescribeClientQuotas and AlterClientQuotas (APIs 48 and 49), behaviours QT-1 to QT-5: set and described, kept across a
//! restart, refused without the cluster's rights. Enforcement is `client_quotas_enforced.rs`; the answers are held to real
//! Kafka 4.4.0's by `kafka-wire/examples/oracle_diff`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Caller;
use kafka_storage::fs::Fs;
use kafka_storage::sim::SimFs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::describe_client_quotas_request::ComponentData;
use kafka_wire::messages::{
    AlterClientQuotasResponse, DescribeClientQuotasRequest, DescribeClientQuotasResponse,
};
use support::acls::{ALLOW, CLUSTER, LITERAL, closed_world, creation, grant, user};
use support::quotas::{Change, DEFAULT_CLIENT, Entity, alter, alter_as};
use support::{Rig, open_on, rig, rig_with};

/// An entity as `type=name` pairs, and its values.
type Listed = (Vec<String>, Vec<(String, f64)>);

async fn describe_as(
    rig: &Rig,
    who: &Caller,
    components: &[(&str, i8, Option<&str>)],
    strict: bool,
) -> DescribeClientQuotasResponse {
    let mut req = DescribeClientQuotasRequest::default();
    req.strict = strict;
    req.components = (components.iter())
        .map(|(t, m, n)| {
            let mut c = ComponentData::default();
            c.entity_type = StrBytes::from_string((*t).to_owned());
            c.match_type = *m;
            c._match = n.map(|n| StrBytes::from_string(n.to_owned()));
            c
        })
        .collect();
    let Response::DescribeClientQuotas(r) =
        rig.ask_as(who, 1, Request::DescribeClientQuotas(req)).await
    else {
        panic!("not DescribeClientQuotas")
    };
    r
}

/// Every entity described, as `type=name` pairs, with its values.
fn listed(r: &DescribeClientQuotasResponse) -> Vec<Listed> {
    (r.entries.iter().flatten())
        .map(|e| {
            let entity = (e.entity.iter())
                .map(|p| {
                    format!(
                        "{}={}",
                        p.entity_type,
                        p.entity_name.as_deref().unwrap_or("<default>")
                    )
                })
                .collect();
            let values = e
                .values
                .iter()
                .map(|v| (v.key.to_string(), v.value))
                .collect();
            (entity, values)
        })
        .collect()
}

fn codes(r: &AlterClientQuotasResponse) -> Vec<i16> {
    r.entries.iter().map(|e| e.error_code).collect()
}

const ALICE: Entity<'static> = &[("user", Some("alice"))];

#[tokio::test]
async fn quotas_set_are_described_and_kept_across_a_restart() {
    let fs = SimFs::new();
    fs.create_dir(std::path::Path::new("/")).ok();
    let rig = open_on(fs.clone());
    let me = support::caller();
    let set: &[Change<'_>] = &[
        (
            ALICE,
            &[
                ("producer_byte_rate", Some(1024.0)),
                ("consumer_byte_rate", Some(2048.0)),
            ],
        ),
        (DEFAULT_CLIENT, &[("request_percentage", Some(12.5))]),
    ];
    // Validate-only changes nothing.
    assert_eq!(codes(&alter_as(&rig, &me, alter(set, true)).await), [0, 0]);
    assert!(listed(&describe_as(&rig, &me, &[], false).await).is_empty());
    assert_eq!(codes(&alter_as(&rig, &me, alter(set, false)).await), [0, 0]);
    // By entity (an entity with no user part first), the values by key.
    let want = vec![
        (
            vec!["client-id=<default>".to_owned()],
            vec![("request_percentage".to_owned(), 12.5)],
        ),
        (
            vec!["user=alice".to_owned()],
            vec![
                ("consumer_byte_rate".to_owned(), 2048.0),
                ("producer_byte_rate".to_owned(), 1024.0),
            ],
        ),
    ];
    assert_eq!(listed(&describe_as(&rig, &me, &[], false).await), want);
    rig.broker.shutdown().await.unwrap();
    drop(rig);
    let again = open_on(fs);
    assert_eq!(listed(&describe_as(&again, &me, &[], false).await), want);
    // Removing every key of an entity removes it, durably too.
    let clear: &[Change<'_>] = &[(DEFAULT_CLIENT, &[("request_percentage", None)])];
    assert_eq!(
        codes(&alter_as(&again, &me, alter(clear, false)).await),
        [0]
    );
    assert_eq!(
        listed(&describe_as(&again, &me, &[("client-id", 2, None)], false).await),
        vec![]
    );
}

#[tokio::test]
async fn a_refused_entry_is_invalid_request_with_kafkas_words_and_the_others_stand() {
    let rig = rig();
    let me = support::caller();
    let req: &[Change<'_>] = &[
        (
            &[("user", Some("bob"))],
            &[("producer_byte_rate", Some(1.5))],
        ),
        (ALICE, &[("producer_byte_rate", Some(10.0))]),
    ];
    let r = alter_as(&rig, &me, alter(req, false)).await;
    assert_eq!(codes(&r), [ResponseError::InvalidRequest.code(), 0]);
    assert_eq!(
        r.entries[0].error_message.as_deref(),
        Some("producer_byte_rate cannot be a fractional value.")
    );
    assert_eq!(r.entries[1].error_message, None);
    assert_eq!(listed(&describe_as(&rig, &me, &[], false).await).len(), 1);
}

#[tokio::test]
async fn altering_needs_alter_configs_and_describing_describe_configs_on_the_cluster() {
    let rig = rig_with(closed_world);
    let refused = alter_as(
        &rig,
        &user("someone"),
        alter(&[(ALICE, &[("producer_byte_rate", Some(1.0))])], false),
    )
    .await;
    assert_eq!(
        codes(&refused),
        [ResponseError::ClusterAuthorizationFailed.code()]
    );
    assert_eq!(
        refused.entries[0].error_message.as_deref(),
        Some("Cluster authorization failed.")
    );
    assert_eq!(
        refused.entries[0].entity[0].entity_name.as_deref(),
        Some("alice")
    );
    let d = describe_as(&rig, &user("someone"), &[], false).await;
    assert_eq!(
        d.error_code,
        ResponseError::ClusterAuthorizationFailed.code()
    );
    assert_eq!(
        (d.error_message.as_deref(), d.entries.is_none()),
        (Some("Cluster authorization failed."), true)
    );
    // DESCRIBE_CONFIGS (10) lets one see, not change; ALTER_CONFIGS (11) lets one change.
    let cluster = |p: &str, op| creation((CLUSTER, "kafka-cluster", LITERAL), p, op, ALLOW);
    grant(
        &rig,
        vec![cluster("User:reader", 10), cluster("User:writer", 11)],
    )
    .await;
    assert_eq!(
        describe_as(&rig, &user("reader"), &[], false)
            .await
            .error_code,
        0
    );
    let by_reader = alter_as(
        &rig,
        &user("reader"),
        alter(&[(ALICE, &[("producer_byte_rate", Some(1.0))])], false),
    )
    .await;
    assert_eq!(
        codes(&by_reader),
        [ResponseError::ClusterAuthorizationFailed.code()]
    );
    let by_writer = alter_as(
        &rig,
        &user("writer"),
        alter(&[(ALICE, &[("producer_byte_rate", Some(1.0))])], false),
    )
    .await;
    assert_eq!(codes(&by_writer), [0]);
    // ALTER_CONFIGS implies DESCRIBE_CONFIGS, as in Kafka's authorizer.
    assert_eq!(
        describe_as(&rig, &user("writer"), &[], false)
            .await
            .error_code,
        0
    );
}
