//! Metadata under ACLs, part one: what a caller may DESCRIBE is what it sees (kafka-semantics 2.1, KafkaApis.scala 880-1002),
//! and the recorded `sasl-plain` identity probe.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Caller;
use support::acls::{DESCRIBE, closed_world, grant, on, shown, user};
use support::{Rig, rig_with, topic};

/// The fixture of `sasl-plain`: for-alice and for-bob, each described by its own user.
async fn fixture() -> Rig {
    let rig = rig_with(closed_world);
    rig.create(vec![topic("for-alice", 1), topic("for-bob", 1)], false)
        .await;
    grant(
        &rig,
        vec![
            on("for-alice", "User:alice", DESCRIBE),
            on("for-bob", "User:bob", DESCRIBE),
        ],
    )
    .await;
    rig
}

#[tokio::test]
async fn the_recorded_identity_probe() {
    let rig = fixture().await;
    let ask = |who: Caller| {
        let rig = &rig;
        async move {
            rig.metadata_as(&who, 9, Some(&["for-alice", "for-bob"]), false)
                .await
        }
    };
    // alice: for-alice is described, for-bob is 29 with its name kept, no partitions, a zero topic id.
    let r = ask(user("alice")).await;
    assert_eq!(
        shown(&r),
        [("for-alice".into(), 0, 1), ("for-bob".into(), 29, 0)]
    );
    let denied = &r.topics[1];
    assert!(denied.topic_id.is_nil() && !denied.is_internal);
    assert!(!r.topics[0].topic_id.is_nil());
    assert_eq!(r.brokers.len(), 1, "the brokers are there whoever asks");
    // bob: the other way round. Kafka lists what is authorized first, then what is denied.
    let r = ask(user("bob")).await;
    assert_eq!(
        shown(&r),
        [("for-bob".into(), 0, 1), ("for-alice".into(), 29, 0)]
    );
    // A caller with no ACL at all gets 29 for both (the telemetry-acl probe), and a super user sees both.
    assert_eq!(
        shown(&ask(user("carol")).await),
        [("for-alice".into(), 29, 0), ("for-bob".into(), 29, 0)]
    );
    assert_eq!(
        shown(&ask(user("admin")).await),
        [("for-alice".into(), 0, 1), ("for-bob".into(), 0, 1)]
    );
}

#[tokio::test]
async fn a_topic_that_does_not_exist_is_hidden_the_same_way() {
    let rig = fixture().await;
    let r = rig
        .metadata_as(&user("carol"), 12, Some(&["for-bob", "nowhere"]), false)
        .await;
    // Denied alike for the one that exists and the one that does not: its existence is not told.
    assert_eq!(
        shown(&r),
        [("for-bob".into(), 29, 0), ("nowhere".into(), 29, 0)]
    );
    // For a caller that may describe it, a missing one is simply missing.
    grant(&rig, vec![on("nowhere", "User:carol", DESCRIBE)]).await;
    let r = rig
        .metadata_as(&user("carol"), 12, Some(&["nowhere"]), false)
        .await;
    assert_eq!(shown(&r), [("nowhere".into(), 3, 0)]);
}

#[tokio::test]
async fn a_list_of_all_topics_shows_only_those_the_caller_may_describe() {
    let rig = fixture().await;
    for (who, want) in [
        ("alice", vec!["for-alice"]),
        ("bob", vec!["for-bob"]),
        ("carol", vec![]),
        ("admin", vec!["for-alice", "for-bob"]),
    ] {
        for version in [0, 12] {
            let all = rig.metadata_as(&user(who), version, None, false).await;
            let mut names: Vec<_> = shown(&all).into_iter().map(|t| t.0).collect();
            names.sort();
            assert_eq!(names, want, "{who} v{version}");
            assert!(
                all.topics.iter().all(|t| t.error_code == 0),
                "no denied entries in a list of all"
            );
        }
    }
}

#[tokio::test]
async fn by_id_a_denied_topic_is_29_with_the_id_and_no_name() {
    let rig = fixture().await;
    let id = rig
        .metadata_as(&user("admin"), 12, Some(&["for-bob"]), false)
        .await
        .topics[0]
        .topic_id;
    let mut req = kafka_wire::messages::MetadataRequest::default();
    let mut t = kafka_wire::messages::metadata_request::MetadataRequestTopic::default();
    t.name = None;
    t.topic_id = id;
    req.topics = Some(vec![t]);
    let ask = |who: Caller, req: kafka_wire::messages::MetadataRequest| {
        let rig = &rig;
        async move {
            let kafka_wire::api::Response::Metadata(r) = rig
                .ask_as(&who, 12, kafka_wire::api::Request::Metadata(req))
                .await
            else {
                panic!()
            };
            r
        }
    };
    let r = ask(user("alice"), req.clone()).await;
    assert_eq!(
        (
            r.topics[0].error_code,
            r.topics[0].name.is_none(),
            r.topics[0].topic_id
        ),
        (29, true, id)
    );
    let r = ask(user("bob"), req).await;
    assert_eq!(
        (r.topics[0].error_code, r.topics[0].partitions.len()),
        (0, 1)
    );
}
