//! The client quotas enforced on the requests that come in through `Broker::serve` (a client's), and not on those through
//! `Broker::handle` (the console's): behaviours QT-8 to QT-12. The arithmetic is held to Kafka's own in `quotas::meter::tests`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::{Caller, Outcome, QuotaKind, Served};
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::{ApiKey, CreateTopicsRequest, FetchRequest};
use support::quotas::{Change, DEFAULT_CLIENT, alter, alter_as};
use support::{Rig, at, produce_request, records, rig, topic, wire_name};

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
    head: RequestHead,
    request: Request,
    size: usize,
) -> Served {
    rig.broker.serve(&at(), who, &head, request, size).await
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
        head(ApiKey::Produce, 9, "p"),
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
    let set: &[Change<'_>] = &[(
        &[("client-id", Some("p"))],
        &[("producer_byte_rate", Some(1000.0))],
    )];
    alter_as(&rig, &support::caller(), alter(set, false)).await;
    let anyone = support::caller();
    let produce = || Request::Produce(produce_request(-1, "orders", &[(0, records(1))]));
    // 50 000 bytes over a span of ten seconds is 5 000/s: four times over, for about 40 s.
    let served = serve(
        &rig,
        &anyone,
        head(ApiKey::Produce, 9, "p"),
        produce(),
        50_000,
    )
    .await;
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
    let other = serve(
        &rig,
        &anyone,
        head(ApiKey::Produce, 9, "q"),
        produce(),
        50_000,
    )
    .await;
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
    let set: &[Change<'_>] = &[(
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
        head(ApiKey::Fetch, 12, "c"),
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
    let set: &[Change<'_>] = &[(DEFAULT_CLIENT, &[("controller_mutation_rate", Some(1.0))])];
    alter_as(&rig, &support::caller(), alter(set, false)).await;
    let create = |names: &[&str]| {
        let mut req = CreateTopicsRequest::default();
        req.topics = names.iter().map(|n| topic(n, 5)).collect();
        Request::CreateTopics(req)
    };
    let served = serve(
        &rig,
        &support::caller(),
        head(ApiKey::CreateTopics, 7, "admin"),
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
        head(ApiKey::CreateTopics, 5, "admin"),
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
