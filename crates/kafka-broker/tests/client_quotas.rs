//! DescribeClientQuotas and AlterClientQuotas (APIs 48 and 49), and the quotas held to as Kafka holds them (behaviours T-Q*):
//! set and described, kept across a restart, refused without the cluster's rights, and enforced on the requests that come in
//! through `Broker::serve` (a client's) and not on those through `Broker::handle` (the console's). The quota arithmetic is
//! held to Kafka's own in `quotas::meter::tests`; the answers to real Kafka 4.4.0's by `kafka-wire/examples/oracle_diff.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::{Caller, Outcome, QuotaKind, Served};
use kafka_storage::fs::Fs;
use kafka_storage::sim::SimFs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::alter_client_quotas_request::{EntityData, EntryData, OpData};
use kafka_wire::messages::describe_client_quotas_request::ComponentData;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::{
    AlterClientQuotasRequest, AlterClientQuotasResponse, ApiKey, CreateTopicsRequest,
    DescribeClientQuotasRequest, DescribeClientQuotasResponse, FetchRequest,
};
use support::acls::{ALLOW, CLUSTER, LITERAL, closed_world, creation, grant, user};
use support::{Rig, at, open_on, produce_request, records, rig, rig_with, topic, wire_name};

type Entity<'a> = &'a [(&'a str, Option<&'a str>)];

fn alter(
    entries: &[(Entity<'_>, &[(&str, Option<f64>)])],
    validate_only: bool,
) -> AlterClientQuotasRequest {
    let mut req = AlterClientQuotasRequest::default();
    req.validate_only = validate_only;
    for (entity, ops) in entries {
        let mut e = EntryData::default();
        e.entity = (entity.iter())
            .map(|(t, n)| {
                let mut part = EntityData::default();
                part.entity_type = StrBytes::from_string((*t).to_owned());
                part.entity_name = n.map(|n| StrBytes::from_string(n.to_owned()));
                part
            })
            .collect();
        e.ops = (ops.iter())
            .map(|(k, v)| {
                let mut op = OpData::default();
                op.key = StrBytes::from_string((*k).to_owned());
                op.value = v.unwrap_or(0.0);
                op.remove = v.is_none();
                op
            })
            .collect();
        req.entries.push(e);
    }
    req
}

async fn alter_as(
    rig: &Rig,
    who: &Caller,
    req: AlterClientQuotasRequest,
) -> AlterClientQuotasResponse {
    let Response::AlterClientQuotas(r) = rig.ask_as(who, 1, Request::AlterClientQuotas(req)).await
    else {
        panic!("not AlterClientQuotas")
    };
    r
}

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
fn listed(r: &DescribeClientQuotasResponse) -> Vec<(Vec<String>, Vec<(String, f64)>)> {
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
const DEFAULT_CLIENT: Entity<'static> = &[("client-id", None)];

#[tokio::test]
async fn quotas_set_are_described_and_kept_across_a_restart() {
    let fs = SimFs::new();
    fs.create_dir(std::path::Path::new("/")).ok();
    let rig = open_on(fs.clone());
    let me = support::caller();
    let set: &[(Entity<'_>, &[(&str, Option<f64>)])] = &[
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
    let clear: &[(Entity<'_>, &[(&str, Option<f64>)])] =
        &[(DEFAULT_CLIENT, &[("request_percentage", None)])];
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
    let req: &[(Entity<'_>, &[(&str, Option<f64>)])] = &[
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

fn head(key: ApiKey, version: i16, client: &str) -> RequestHead {
    RequestHead {
        key,
        version,
        correlation_id: 1,
        client_id: Some(StrBytes::from_string(client.to_owned())),
    }
}

async fn serve(
    rig: &Rig,
    who: &Caller,
    key: ApiKey,
    version: i16,
    client: &str,
    request: Request,
    size: usize,
) -> Served {
    rig.broker
        .serve(&at(), who, &head(key, version, client), request, size)
        .await
}

fn throttle_of(outcome: &Outcome) -> i32 {
    match outcome {
        Outcome::Respond(Response::Produce(r)) => r.throttle_time_ms,
        Outcome::Respond(Response::Fetch(r)) => r.throttle_time_ms,
        Outcome::Respond(Response::CreateTopics(r)) => r.throttle_time_ms,
        Outcome::Respond(Response::Metadata(r)) => r.throttle_time_ms,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn with_no_quota_a_clients_request_is_served_as_the_consoles_and_never_muted() {
    let rig = rig();
    rig.create(vec![topic("orders", 1)], false).await;
    let req = produce_request(-1, "orders", &[(0, records(10))]);
    let served = serve(
        &rig,
        &support::caller(),
        ApiKey::Produce,
        9,
        "p",
        Request::Produce(req),
        10_000_000,
    )
    .await;
    assert!(served.mute.is_none());
    assert_eq!(throttle_of(&served.outcome), 0);
}

#[tokio::test]
async fn a_producer_over_its_byte_rate_is_answered_with_the_throttle_and_muted_for_it() {
    let rig = rig();
    rig.create(vec![topic("orders", 1)], false).await;
    let set: &[(Entity<'_>, &[(&str, Option<f64>)])] = &[(
        &[("client-id", Some("p"))],
        &[("producer_byte_rate", Some(1000.0))],
    )];
    alter_as(&rig, &support::caller(), alter(set, false)).await;
    let anyone = support::caller();
    let produce = || Request::Produce(produce_request(-1, "orders", &[(0, records(1))]));
    // 50 000 bytes over a span of ten seconds is 5 000/s: four times over, for about 40 s.
    let served = serve(&rig, &anyone, ApiKey::Produce, 9, "p", produce(), 50_000).await;
    let throttle = throttle_of(&served.outcome);
    assert!((39_000..=40_001).contains(&throttle), "{throttle}");
    assert_eq!(
        served.mute.map(|d| d.as_millis()),
        Some(u128::try_from(throttle).unwrap())
    );
    // The records were taken all the same.
    let Outcome::Respond(Response::Produce(r)) = served.outcome else {
        panic!()
    };
    assert_eq!(r.responses[0].partition_responses[0].error_code, 0);
    // Another client id is not held to it, and the console never is.
    let other = serve(&rig, &anyone, ApiKey::Produce, 9, "q", produce(), 50_000).await;
    assert!(other.mute.is_none());
    assert!(matches!(
        rig.broker
            .handle(&at(), &anyone, &head(ApiKey::Produce, 9, "p"), produce())
            .await,
        Outcome::Respond(_)
    ));
    // The console sees who is muted.
    let quotas = rig.broker.client_quotas(&anyone).unwrap();
    let p = quotas
        .usage
        .iter()
        .find(|u| u.kind == QuotaKind::Produce && u.client_id == "p")
        .unwrap();
    assert_eq!(p.limit, Some(1000.0));
    assert!(p.throttled_until_ms > quotas.now_ms);
}

#[tokio::test]
async fn a_fetch_over_its_byte_rate_is_answered_empty_and_its_bytes_are_given_back() {
    let rig = rig();
    rig.create(vec![topic("orders", 1)], false).await;
    for _ in 0..20 {
        assert_eq!(
            rig.produce(-1, "orders", 0, records(50)).await.error_code,
            0
        );
    }
    let set: &[(Entity<'_>, &[(&str, Option<f64>)])] = &[(
        &[("client-id", Some("c"))],
        &[("consumer_byte_rate", Some(10.0))],
    )];
    alter_as(&rig, &support::caller(), alter(set, false)).await;
    let fetch = || {
        let mut p = FetchPartition::default();
        p.partition = 0;
        p.partition_max_bytes = 1 << 20;
        let mut t = FetchTopic::default();
        t.topic = wire_name("orders");
        t.partitions.push(p);
        let mut req = FetchRequest::default();
        req.max_bytes = 50 << 20;
        req.topics.push(t);
        Request::Fetch(req)
    };
    let first = serve(
        &rig,
        &support::caller(),
        ApiKey::Fetch,
        12,
        "c",
        fetch(),
        100,
    )
    .await;
    // The answer is cut to what may be sent in one go (10 B/s for ten windows less one: 100 bytes), but a first batch is
    // always sent whole, and a batch of 50 records over the span is past the rate: throttled, and sent nothing.
    assert!(first.mute.is_some());
    let Outcome::Respond(Response::Fetch(r)) = &first.outcome else {
        panic!()
    };
    assert!(r.responses.is_empty() && r.throttle_time_ms > 0, "{r:?}");
}

#[tokio::test]
async fn the_mutation_quota_refuses_the_topic_after_the_one_that_empties_it_from_v6_and_throttles_below()
 {
    let rig = rig();
    let set: &[(Entity<'_>, &[(&str, Option<f64>)])] =
        &[(DEFAULT_CLIENT, &[("controller_mutation_rate", Some(1.0))])];
    alter_as(&rig, &support::caller(), alter(set, false)).await;
    let create = |names: &[&str]| {
        let mut req = CreateTopicsRequest::default();
        req.topics = names.iter().map(|n| topic(n, 5)).collect();
        Request::CreateTopics(req)
    };
    let served = serve(
        &rig,
        &support::caller(),
        ApiKey::CreateTopics,
        7,
        "admin",
        create(&["a", "b", "c", "d"]),
        100,
    )
    .await;
    let Outcome::Respond(Response::CreateTopics(r)) = &served.outcome else {
        panic!()
    };
    let errors: Vec<i16> = r.topics.iter().map(|t| t.error_code).collect();
    assert_eq!(
        errors,
        [0, 0, 0, ResponseError::ThrottlingQuotaExceeded.code()]
    );
    assert_eq!(r.topics[3].error_message, None);
    assert!(
        (3_900..=4_000).contains(&r.throttle_time_ms),
        "{}",
        r.throttle_time_ms
    );
    // Below v6 the topic is made and the client throttled instead.
    let old = serve(
        &rig,
        &support::caller(),
        ApiKey::CreateTopics,
        5,
        "admin",
        create(&["e"]),
        100,
    )
    .await;
    let Outcome::Respond(Response::CreateTopics(r)) = &old.outcome else {
        panic!()
    };
    assert_eq!(r.topics[0].error_code, 0);
    assert!(r.throttle_time_ms > 4_000 && old.mute.is_some());
}
