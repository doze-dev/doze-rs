//! Client telemetry (KIP-714) through the broker: GetTelemetrySubscriptions and PushTelemetry answered as the recordings of
//! Apache Kafka 4.3.1 are (`telemetry-errors`, `-throttle`, `-ask-again`, `-terminating`, `-broker-restart`,
//! `-instance-binding`, `-acl`). The timing rules (an interval of five seconds) are the store's unit tests; here every push
//! is a fresh instance's first, which is never early, or the second of one, which always is.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::io::Write;

use bytes::Bytes;
use kafka_broker::Caller;
use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{
    GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse, PushTelemetryRequest,
};
use support::acls::{ALL, CLUSTER, DENY, LITERAL, creation, grant, user};
use support::{Rig, caller, rig, rig_with};

const INVALID_REQUEST: i16 = 42;
const UNSUPPORTED_COMPRESSION: i16 = 76;
const INVALID_RECORD: i16 = 87;
const THROTTLED: i16 = 89;
const UNKNOWN_SUBSCRIPTION: i16 = 117;
const TOO_LARGE: i16 = 118;
const MAX: usize = 1 << 20;

/// Stands in for what a client pushes; the broker never reads it.
const METRICS: &[u8] = b"not a metrics payload, and the broker does not mind";

async fn get_as(rig: &Rig, who: &Caller, id: Uuid) -> GetTelemetrySubscriptionsResponse {
    let mut req = GetTelemetrySubscriptionsRequest::default();
    req.client_instance_id = id;
    let Response::GetTelemetrySubscriptions(r) = rig
        .ask_as(who, 0, Request::GetTelemetrySubscriptions(req))
        .await
    else {
        panic!("not GetTelemetrySubscriptions")
    };
    r
}

async fn get(rig: &Rig, id: Uuid) -> GetTelemetrySubscriptionsResponse {
    get_as(rig, &caller(), id).await
}

struct Push {
    id: Uuid,
    subscription: i32,
    compression: i8,
    terminating: bool,
    body: Vec<u8>,
}

impl Push {
    fn of(s: &GetTelemetrySubscriptionsResponse) -> Self {
        Self {
            id: s.client_instance_id,
            subscription: s.subscription_id,
            compression: 0,
            terminating: false,
            body: METRICS.to_vec(),
        }
    }
}

async fn push_as(rig: &Rig, who: &Caller, p: &Push) -> i16 {
    let mut req = PushTelemetryRequest::default();
    req.client_instance_id = p.id;
    req.subscription_id = p.subscription;
    req.compression_type = p.compression;
    req.terminating = p.terminating;
    req.metrics = Bytes::from(p.body.clone());
    let Response::PushTelemetry(r) = rig.ask_as(who, 0, Request::PushTelemetry(req)).await else {
        panic!("not PushTelemetry")
    };
    assert_eq!(r.throttle_time_ms, 0);
    r.error_code
}

async fn push(rig: &Rig, p: &Push) -> i16 {
    push_as(rig, &caller(), p).await
}

/// A subscription asked for under a new instance id.
async fn fresh(rig: &Rig) -> GetTelemetrySubscriptionsResponse {
    get(rig, Uuid::nil()).await
}

fn gzipped(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

#[tokio::test]
async fn a_client_with_no_id_is_given_one_and_the_subscription_the_recordings_show() {
    let rig = rig();
    let s = fresh(&rig).await;
    assert_eq!(s.error_code, 0);
    assert!(!s.client_instance_id.is_nil());
    assert_eq!(s.client_instance_id.get_version_num(), 4);
    assert_ne!(s.subscription_id, 0);
    // Kafka's own order of preference (zstd, lz4, gzip, snappy) and limit; five seconds, the standard set, cumulative.
    assert_eq!(s.accepted_compression_types, [4, 3, 1, 2]);
    assert_eq!(s.telemetry_max_bytes, 1_048_576);
    assert_eq!(s.push_interval_ms, 5000);
    assert_eq!(s.requested_metrics.len(), 1);
    assert_eq!(&*s.requested_metrics[0], "org.apache.kafka.");
    assert!(!s.delta_temporality);
    assert_eq!(s.throttle_time_ms, 0);
    let other = fresh(&rig).await;
    assert_ne!(other.client_instance_id, s.client_instance_id);
}

#[tokio::test]
async fn asking_again_at_once_is_throttled_with_nothing_else_in_the_answer() {
    let rig = rig();
    let s = fresh(&rig).await;
    let again = get(&rig, s.client_instance_id).await;
    assert_eq!(again.error_code, THROTTLED);
    assert!(again.client_instance_id.is_nil());
    assert_eq!((again.subscription_id, again.push_interval_ms), (0, 0));
    assert!(again.accepted_compression_types.is_empty() && again.requested_metrics.is_empty());
}

#[tokio::test]
async fn a_subscription_id_is_the_same_after_a_restart_and_a_push_that_holds_it_is_taken() {
    let (before, after) = (rig(), rig());
    let s = fresh(&before).await;
    // The broker the client asked has gone; the new one has never heard of it.
    let mut p = Push::of(&s);
    assert_eq!(push(&after, &p).await, 0);
    // And it is the id of this instance, not of another.
    p.id = fresh(&before).await.client_instance_id;
    assert_eq!(push(&after, &p).await, UNKNOWN_SUBSCRIPTION);
    // The instance the new broker has now heard of is throttled like any other.
    assert_eq!(
        get(&after, s.client_instance_id).await.error_code,
        THROTTLED
    );
}

/// Pushes with `change` made to a push that would be taken, each from an instance of its own, whose first push is never early.
type Change = Box<dyn Fn(&mut Push)>;

async fn answers_to(rig: &Rig, cases: Vec<(&str, Change, i16)>) {
    for (label, change, recorded) in cases {
        let mut p = Push::of(&fresh(rig).await);
        change(&mut p);
        assert_eq!(push(rig, &p).await, recorded, "{label}");
    }
}

fn packed(codec: i8, body: Vec<u8>) -> Change {
    Box::new(move |p| (p.compression, p.body) = (codec, body.clone()))
}

#[tokio::test]
async fn the_refusals_are_the_recorded_ones() {
    answers_to(
        &rig(),
        vec![
            (
                "zero instance id",
                Box::new(|p| p.id = Uuid::nil()),
                INVALID_REQUEST,
            ),
            (
                "unknown subscription id",
                Box::new(|p| p.subscription += 1),
                UNKNOWN_SUBSCRIPTION,
            ),
            (
                "compression 9",
                Box::new(|p| p.compression = 9),
                UNSUPPORTED_COMPRESSION,
            ),
            (
                "compression -1",
                Box::new(|p| p.compression = -1),
                UNSUPPORTED_COMPRESSION,
            ),
            (
                "over the limit",
                Box::new(|p| p.body = vec![0; MAX + 1]),
                TOO_LARGE,
            ),
            ("at the limit", Box::new(|p| p.body = vec![0; MAX]), 0),
        ],
    )
    .await;
}

#[tokio::test]
async fn a_payload_is_judged_by_its_codec_and_never_by_what_it_says() {
    let junk = b"this is not a compressed body".to_vec();
    answers_to(
        &rig(),
        vec![
            (
                "gzip that inflates past the limit",
                packed(1, gzipped(&vec![0; 4 << 20])),
                INVALID_RECORD,
            ),
            ("corrupt gzip", packed(1, junk.clone()), INVALID_RECORD),
            ("corrupt snappy", packed(2, junk.clone()), INVALID_RECORD),
            ("corrupt lz4", packed(3, junk.clone()), INVALID_RECORD),
            ("corrupt zstd", packed(4, junk), INVALID_RECORD),
            (
                "not metrics",
                Box::new(|p| p.body = b"this is not a metrics payload".to_vec()),
                0,
            ),
            (
                "gzip of text",
                packed(1, gzipped(b"this is not a metrics payload")),
                0,
            ),
            ("empty body", Box::new(|p| p.body = Vec::new()), 0),
        ],
    )
    .await;
}

#[tokio::test]
async fn when_a_push_has_several_faults_the_first_in_kafkas_order_is_the_answer() {
    let rig = rig();
    let wrong: fn(&mut Push) = |p| p.subscription += 1;
    let zip: fn(&mut Push) = |p| p.compression = 9;
    let big: fn(&mut Push) = |p| p.body = vec![0; MAX + 1];
    // Recorded in telemetry-instance-binding-spaced: subscription, then compression, then size.
    for (label, changes, recorded) in [
        (
            "wrong subscription and compression",
            vec![wrong, zip],
            UNKNOWN_SUBSCRIPTION,
        ),
        (
            "wrong subscription and too large",
            vec![wrong, big],
            UNKNOWN_SUBSCRIPTION,
        ),
        (
            "compression and too large",
            vec![zip, big],
            UNSUPPORTED_COMPRESSION,
        ),
    ] {
        let mut p = Push::of(&fresh(&rig).await);
        changes.iter().for_each(|c| c(&mut p));
        assert_eq!(push(&rig, &p).await, recorded, "{label}");
    }
}

#[tokio::test]
async fn a_push_straight_after_another_is_early_whatever_else_is_wrong_with_it() {
    let rig = rig();
    let s = fresh(&rig).await;
    let p = Push::of(&s);
    assert_eq!(push(&rig, &p).await, 0);
    assert_eq!(push(&rig, &p).await, THROTTLED);
    // Recorded in telemetry-throttle: the gate comes before every other check.
    for (label, change) in [
        (
            "unknown subscription",
            (|p: &mut Push| p.subscription += 1) as fn(&mut Push),
        ),
        ("unsupported compression", |p| p.compression = 9),
        ("too large", |p| p.body = vec![0; MAX + 1]),
        ("corrupt", |p| {
            (p.compression, p.body) = (1, b"not gzip".to_vec())
        }),
    ] {
        let mut q = Push::of(&s);
        change(&mut q);
        assert_eq!(push(&rig, &q).await, THROTTLED, "{label}");
    }
}

#[tokio::test]
async fn a_terminating_push_is_taken_when_early_and_the_next_is_refused_once() {
    let rig = rig();
    let s = fresh(&rig).await;
    let mut p = Push::of(&s);
    assert_eq!(push(&rig, &p).await, 0);
    p.terminating = true;
    assert_eq!(
        push(&rig, &p).await,
        0,
        "the last word is taken though early"
    );
    p.terminating = false;
    // Recorded: INVALID_REQUEST once, then the interval's throttle again.
    assert_eq!(push(&rig, &p).await, INVALID_REQUEST);
    assert_eq!(push(&rig, &p).await, THROTTLED);
    // A subscription request does not clear it either way: it is early too.
    assert_eq!(get(&rig, s.client_instance_id).await.error_code, THROTTLED);
}

#[tokio::test]
async fn a_refusal_that_tells_a_client_to_ask_again_lets_its_next_request_through_once() {
    let rig = rig();
    for refusal in [
        |p: &mut Push| p.subscription += 1,
        |p: &mut Push| p.compression = 9,
    ] {
        let s = fresh(&rig).await;
        let mut p = Push::of(&s);
        refusal(&mut p);
        assert_ne!(push(&rig, &p).await, 0);
        // Answered though it comes at once, because the client did what it was told.
        let again = get(&rig, s.client_instance_id).await;
        assert_eq!(again.error_code, 0);
        assert_eq!(again.subscription_id, s.subscription_id);
        // Once: the next is early, and so is the push after that request, which sets no clock.
        assert_eq!(get(&rig, s.client_instance_id).await.error_code, THROTTLED);
    }
}

#[tokio::test]
async fn any_connection_may_speak_for_any_instance_and_no_acl_is_asked() {
    // A broker that refuses everything nobody was allowed: recorded `telemetry-acl` and `-instance-binding`.
    let rig = rig_with(support::acls::closed_world);
    let (alice, bob) = (user("alice"), user("bob"));
    // Not even a DENY of everything on the cluster stands in the way: the rows of the authorization matrix say none (SEC-290).
    let barred = creation((CLUSTER, "kafka-cluster", LITERAL), "User:carol", ALL, DENY);
    grant(&rig, vec![barred]).await;
    let carol = user("carol");
    let c = get_as(&rig, &carol, Uuid::nil()).await;
    assert_eq!(push_as(&rig, &carol, &Push::of(&c)).await, 0);
    let s = get_as(&rig, &alice, Uuid::nil()).await;
    assert_eq!(s.error_code, 0);
    let p = Push::of(&s);
    assert_eq!(
        push_as(&rig, &bob, &p).await,
        0,
        "bob pushes alice's instance"
    );
    assert_eq!(
        push_as(&rig, &alice, &p).await,
        THROTTLED,
        "and it was alice's turn that he took"
    );
}

#[tokio::test]
async fn the_two_apis_are_served_unless_the_broker_is_told_not_to() {
    use kafka_wire::messages::ApiKey::{GetTelemetrySubscriptions, PushTelemetry};
    let on = rig();
    assert!(on.broker.client_telemetry());
    assert!(on.broker.served().contains(GetTelemetrySubscriptions));
    assert!(on.broker.served().contains(PushTelemetry));
    let off = rig_with(|c| c.client_telemetry = false);
    assert!(!off.broker.client_telemetry());
    assert!(!off.broker.served().contains(GetTelemetrySubscriptions));
    assert!(!off.broker.served().contains(PushTelemetry));
}
