//! Which Metadata entries carry the authorized-operations bits, and where the entries go. Source: `KafkaApis.scala` 973-985
//! (SEC-214, SEC-215, SEC-D83): the bits are set on `topicMetadata` only; the entries refused for CREATE or DESCRIBE and the topic ids
//! nobody knows are added afterwards (the unknown ids ahead of everything, the refused ones at the end) and keep the schema's
//! default.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use kafka_broker::Caller;
use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::{MetadataRequest, MetadataResponse};
use support::acls::{ALL, ALLOW, CLUSTER, CREATE, DESCRIBE, LITERAL, READ, creation, user};
use support::{Rig, wire_name};
use topics::*;

const DEFAULT: i32 = i32::MIN;

async fn ask(
    rig: &Rig,
    who: &Caller,
    version: i16,
    topics: Vec<MetadataRequestTopic>,
) -> MetadataResponse {
    let mut req = MetadataRequest::default();
    req.allow_auto_topic_creation = true;
    req.include_topic_authorized_operations = true;
    req.topics = Some(topics);
    let Response::Metadata(r) = rig.ask_as(who, version, Request::Metadata(req)).await else {
        panic!("not Metadata")
    };
    r
}

fn by_name(name: &str) -> MetadataRequestTopic {
    let mut t = MetadataRequestTopic::default();
    t.name = Some(wire_name(name));
    t
}

fn by_id(id: Uuid) -> MetadataRequestTopic {
    let mut t = MetadataRequestTopic::default();
    t.name = None;
    t.topic_id = id;
    t
}

/// The auditor's reproduction: READ allowed and DESCRIBE denied: the entry is 29 and has no bits.
#[tokio::test]
async fn an_entry_refused_for_describe_keeps_the_default_authorized_operations() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:alice", READ),
            deny("t", "User:alice", DESCRIBE),
        ],
    )
    .await;
    for version in 8..=12 {
        let r = ask(&rig, &user("alice"), version, vec![by_name("t")]).await;
        assert_eq!(r.topics[0].error_code, 29);
        assert_eq!(
            r.topics[0].topic_authorized_operations, DEFAULT,
            "v{version}"
        );
    }
}

/// An entry refused for CREATE (a topic that is not there, asked for by one who may describe it) is appended after the
/// topic found, with no bits; the one found has them.
#[tokio::test]
async fn an_entry_refused_for_create_has_none_and_the_topic_found_has_its_own() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:alice", READ),
            allow("ghost", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let r = ask(
        &rig,
        &user("alice"),
        12,
        vec![by_name("ghost"), by_name("t")],
    )
    .await;
    let seen: Vec<_> = r
        .topics
        .iter()
        .map(|t| {
            (
                t.name.as_ref().unwrap().0.to_string(),
                t.error_code,
                t.topic_authorized_operations,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("t".to_owned(), 0, (1 << 3) | (1 << 8)),
            ("ghost".to_owned(), 29, DEFAULT)
        ]
    );
}

/// A topic that is not there and may be created (so it is answered 3) is among the entries `topicMetadata` holds: it has
/// the bits of its name.
#[tokio::test]
async fn an_entry_for_a_topic_not_there_has_the_bits_of_its_name() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("ghost", "User:alice", DESCRIBE),
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:alice",
                CREATE,
                ALLOW,
            ),
        ],
    )
    .await;
    let r = ask(&rig, &user("alice"), 12, vec![by_name("ghost")]).await;
    assert_eq!(r.topics[0].error_code, 3);
    assert_eq!(r.topics[0].topic_authorized_operations, 1 << 8);
}

/// Topic ids nobody knows are listed first, whatever order they were asked in (`KafkaApis.scala` 983-985); the ones
/// found follow, then the ones refused.
#[tokio::test]
async fn unknown_topic_ids_come_first() {
    let rig = rig_with_topics(&["t", "hidden"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:alice", ALL),
            allow("hidden", "User:bob", ALL),
        ],
    )
    .await;
    let id = |name: &'static str| {
        let rig = &rig;
        async move {
            rig.metadata_as(&support::acls::admin(), 12, Some(&[name]), false)
                .await
                .topics[0]
                .topic_id
        }
    };
    let (t, hidden) = (id("t").await, id("hidden").await);
    let ghost = Uuid::from_u128(0x1234_5678);
    let r = ask(
        &rig,
        &user("alice"),
        12,
        vec![by_id(hidden), by_id(t), by_id(ghost)],
    )
    .await;
    let seen: Vec<(i16, Uuid, i32)> = r
        .topics
        .iter()
        .map(|e| (e.error_code, e.topic_id, e.topic_authorized_operations))
        .collect();
    assert_eq!(
        seen.iter().map(|s| s.0).collect::<Vec<_>>(),
        [100, 0, 29],
        "{seen:?}"
    );
    assert_eq!(seen[0].1, ghost);
    assert_eq!(seen[0].2, DEFAULT);
    assert_eq!(seen[2], (29, hidden, DEFAULT));
}
