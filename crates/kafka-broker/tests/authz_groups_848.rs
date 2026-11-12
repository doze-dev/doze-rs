//! Authorization of ConsumerGroupHeartbeat and ConsumerGroupDescribe (KIP-848). No row is recorded: each test cites
//! the Kafka 4.3.1 source it is written from (`core/src/main/scala/kafka/server/KafkaApis.scala`, "KA" below).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[macro_use]
#[path = "support/authz_groups.rs"]
mod gx;
#[path = "support/authz_calls.rs"]
mod calls;

use std::time::Duration;

use gx::{DESCRIBE, GROUP, GROUP_AUTH, READ, TOPIC, TOPIC_AUTH, allow, make_topic, need};
use support::acls::{admin, grant, user};
use support::{Rig, beat_848, join_848, rig_with};

/// A broker that refuses by default, whose assignments are computed at every heartbeat.
fn world() -> Rig {
    rig_with(|c| {
        gx::closed(c);
        c.consumer_assignment_interval = Duration::ZERO;
    })
}

/// A group `g` of the super user's member `m1` on topic `t`.
async fn group_with_a_member(rig: &Rig) {
    make_topic(rig, "t", 3).await;
    let r = calls::heartbeat_848(rig, &admin(), 1, join_848("g", "m1", &["t"])).await;
    assert_eq!(r.error_code, 0);
}

fn member_of(who: &kafka_broker::Caller) -> String {
    format!("m-{}", who.principal().as_str())
}

// ---- ConsumerGroupHeartbeat (KA:2604-2642: READ on the group, DESCRIBE on every subscribed topic)

#[tokio::test]
async fn the_heartbeat_needs_read_on_the_group_and_describe_on_each_subscribed_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", DESCRIBE, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let req = join_848("grp", &member_of(who), &["tpc"]);
            calls::heartbeat_848(rig, who, 1, req).await.error_code
        }
    );
}

#[tokio::test]
async fn a_refused_heartbeat_is_the_code_alone_in_both_versions_and_makes_no_member() {
    let rig = world();
    make_topic(&rig, "t", 1).await;
    grant(&rig, vec![allow(GROUP, "g", "User:alice", READ)]).await;
    for version in 0..=1 {
        // No group right: 30, with no message (the exception's message is the default one, which Kafka leaves out).
        let r = calls::heartbeat_848(&rig, &user("bob"), version, join_848("g", "m", &["t"])).await;
        assert_eq!(
            (r.error_code, r.error_message.clone()),
            (30, None),
            "v{version}"
        );
        assert!(
            r.member_id.is_none() && r.assignment.is_none(),
            "v{version}"
        );
        assert_eq!(r.member_epoch, 0);
        // The group right but not DESCRIBE on the topic: 29, and no message either.
        let r =
            calls::heartbeat_848(&rig, &user("alice"), version, join_848("g", "m", &["t"])).await;
        assert_eq!(
            (r.error_code, r.error_message.clone()),
            (29, None),
            "v{version}"
        );
    }
    // Nobody joined.
    let described = calls::describe_848(&rig, &admin(), 1, &["g"], false).await;
    assert_eq!(described[0].error_code, 69);
}

#[tokio::test]
async fn one_topic_the_caller_may_not_describe_among_several_refuses_the_subscription() {
    let rig = world();
    make_topic(&rig, "t", 1).await;
    make_topic(&rig, "u", 1).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "t", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let alice = user("alice");
    let r = calls::heartbeat_848(&rig, &alice, 1, join_848("g", "m", &["t", "u"])).await;
    assert_eq!(r.error_code, 29);
    let joined = calls::heartbeat_848(&rig, &alice, 1, join_848("g", "m", &["t"])).await;
    assert_eq!(joined.error_code, 0);
    // A heartbeat that names no topics (the member keeps on, or the subscription is empty) checks no topic.
    let r = calls::heartbeat_848(
        &rig,
        &alice,
        1,
        beat_848("g", "m", joined.member_epoch, None),
    )
    .await;
    assert_eq!(r.error_code, 0);
    let r = calls::heartbeat_848(&rig, &alice, 1, join_848("g", "m2", &[])).await;
    assert_eq!(r.error_code, 0);
}

#[tokio::test]
async fn authorization_comes_before_the_request_is_judged() {
    // KA:2604-2615: the group is authorized before the coordinator sees the request, so a request that is wrong in itself
    // (an empty member id: INVALID_REQUEST, 42) is 30 to a caller that may not use the group.
    let rig = world();
    grant(&rig, vec![allow(GROUP, "g", "User:alice", READ)]).await;
    let wrong = || join_848("g", "", &[]);
    assert_eq!(
        calls::heartbeat_848(&rig, &user("bob"), 1, wrong())
            .await
            .error_code,
        30
    );
    assert_eq!(
        calls::heartbeat_848(&rig, &user("alice"), 1, wrong())
            .await
            .error_code,
        42
    );
}

// ---- ConsumerGroupDescribe (KA:2644-2728)

#[tokio::test]
async fn the_describe_needs_describe_on_the_group() {
    five_ways!(&[need(GROUP, "grp", DESCRIBE, GROUP_AUTH)], |rig, who| {
        calls::describe_848(rig, who, 1, &["grp"], false).await[0].error_code
    });
}

#[tokio::test]
async fn refused_groups_come_first_and_the_rest_follow() {
    let rig = world();
    group_with_a_member(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", DESCRIBE),
            allow(TOPIC, "t", "User:alice", DESCRIBE),
        ],
    )
    .await;
    for version in 0..=1 {
        let all = calls::describe_848(&rig, &user("alice"), version, &["a", "g", "b"], false).await;
        let seen: Vec<_> = all
            .iter()
            .map(|d| (d.group_id.as_str(), d.error_code))
            .collect();
        assert_eq!(seen, [("a", 30), ("b", 30), ("g", 0)], "v{version}");
        assert!(all[0].members.is_empty() && all[0].error_message.is_none());
    }
}

#[tokio::test]
async fn a_group_holding_a_topic_the_caller_may_not_describe_becomes_a_refusal_with_kafkas_words() {
    let rig = world();
    group_with_a_member(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", DESCRIBE),
            allow(GROUP, "g", "User:carol", DESCRIBE),
            allow(TOPIC, "t", "User:carol", DESCRIBE),
        ],
    )
    .await;
    let d = &calls::describe_848(&rig, &user("alice"), 1, &["g"], false).await[0];
    assert_eq!(d.error_code, 29);
    assert_eq!(
        d.error_message.as_ref().map(|m| m.as_str()),
        Some("The group has described topic(s) that the client is not authorized to describe.")
    );
    assert!(d.members.is_empty());
    assert_eq!(d.group_id.as_str(), "g");
    // One who may describe the topic sees the group whole.
    let d = &calls::describe_848(&rig, &user("carol"), 1, &["g"], false).await[0];
    assert_eq!((d.error_code, d.members.len()), (0, 1));
}

#[tokio::test]
async fn the_operations_are_given_when_asked_and_only_for_a_group_that_is_not_an_error() {
    let rig = world();
    group_with_a_member(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(TOPIC, "t", "User:alice", DESCRIBE),
            allow(GROUP, "g", "User:bob", DESCRIBE),
        ],
    )
    .await;
    let alice = user("alice");
    // KA:2675-2684, AH:62-76: READ is bit 3, and implies DESCRIBE, bit 8.
    let d = &calls::describe_848(&rig, &alice, 1, &["g"], true).await[0];
    assert_eq!(d.authorized_operations, (1 << 3) | (1 << 8));
    let d = &calls::describe_848(&rig, &alice, 1, &["g"], false).await[0];
    assert_eq!(d.authorized_operations, i32::MIN);
    let d = &calls::describe_848(&rig, &admin(), 1, &["g"], true).await[0];
    assert_eq!(d.authorized_operations, 8 + 64 + 256 + 1024 + 2048);
    // The group that became an error has none.
    let d = &calls::describe_848(&rig, &user("bob"), 1, &["g"], true).await[0];
    assert_eq!((d.error_code, d.authorized_operations), (29, i32::MIN));
}
