//! Consumer groups of the new protocol (KIP-848) through the broker: the machine is tested alone, this checks the
//! wiring, the wire at every version, and how the other group APIs see such a group.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::Uuid;
use support::{assigned_848, beat_848, join_848, rig_with, topic};

const INVALID_REQUEST: i16 = 42;
const GROUP_ID_NOT_FOUND: i16 = 69;
const UNKNOWN_MEMBER_ID: i16 = 25;
const FENCED_MEMBER_EPOCH: i16 = 110;
const NON_EMPTY_GROUP: i16 = 68;
const UNSUPPORTED_VERSION: i16 = 35;
const STALE_MEMBER_EPOCH: i16 = 113;
const SUBSCRIBED: i16 = 86;

/// A rig whose assignments are computed at every heartbeat, so that a test does not wait out the interval; topic `t`
/// has three partitions.
async fn rig() -> (support::Rig, Uuid) {
    let rig = rig_with(|c| c.consumer_assignment_interval = Duration::ZERO);
    rig.create(vec![topic("t", 3)], false).await;
    let id = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
    (rig, id)
}

#[tokio::test]
async fn a_member_joins_keeps_on_and_leaves_at_both_versions() {
    for version in 0..=1 {
        let (rig, t) = rig().await;
        let g = format!("g{version}");
        let joined = rig.heartbeat_848(version, join_848(&g, "m1", &["t"])).await;
        assert_eq!(
            (joined.error_code, joined.member_epoch),
            (0, 2),
            "v{version}"
        );
        assert_eq!(joined.member_id.as_ref().unwrap().as_str(), "m1");
        assert_eq!(joined.heartbeat_interval_ms, 1000);
        assert_eq!(assigned_848(&joined), Some(vec![(t, 0), (t, 1), (t, 2)]));
        // Keeping on, saying nothing has changed: no assignment is sent again.
        let again = rig
            .heartbeat_848(version, beat_848(&g, "m1", 2, None))
            .await;
        assert_eq!(
            (again.error_code, again.member_epoch, assigned_848(&again)),
            (0, 2, None)
        );
        // Leaving is answered with the epoch it left at, and no interval.
        let left = rig
            .heartbeat_848(version, beat_848(&g, "m1", -1, None))
            .await;
        assert_eq!(
            (
                left.error_code,
                left.member_epoch,
                left.heartbeat_interval_ms
            ),
            (0, -1, 0)
        );
    }
}

#[tokio::test]
async fn the_group_is_described_with_its_members_and_what_each_holds_at_both_versions() {
    let (rig, t) = rig().await;
    rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    for version in 0..=1 {
        // Answered in Kafka's order, not the request's.
        let all = rig.describe_848(version, &["g", "nobody"]).await;
        let by = |id: &str| {
            all.iter()
                .find(|d| d.group_id.0.as_str() == id)
                .unwrap()
                .clone()
        };
        let d = [by("g"), by("nobody")];
        assert_eq!(
            (
                d[0].error_code,
                d[0].group_state.as_str(),
                d[0].error_message.clone()
            ),
            (0, "Stable", None),
            "v{version}"
        );
        assert_eq!(
            (
                d[0].group_epoch,
                d[0].assignment_epoch,
                d[0].assignor_name.as_str()
            ),
            (2, 2, "uniform")
        );
        let m = &d[0].members[0];
        assert_eq!((m.member_id.as_str(), m.member_epoch), ("m1", 2));
        assert_eq!(m.assignment.topic_partitions[0].topic_id, t);
        assert_eq!(m.assignment.topic_partitions[0].topic_name.0.as_str(), "t");
        assert_eq!(m.assignment.topic_partitions[0].partitions, [0, 1, 2]);
        assert_eq!(
            (
                d[1].error_code,
                d[1].error_message.as_ref().unwrap().as_str()
            ),
            (GROUP_ID_NOT_FOUND, "Group nobody not found.")
        );
    }
}

#[tokio::test]
async fn what_a_client_gets_wrong_is_refused_in_kafkas_words() {
    let (rig, _) = rig().await;
    let words = |r: &kafka_wire::messages::ConsumerGroupHeartbeatResponse| {
        (
            r.error_code,
            r.error_message.as_ref().map(|m| m.as_str().to_owned()),
        )
    };
    let mut empty_instance = join_848("g", "m1", &["t"]);
    empty_instance.instance_id = Some(kafka_wire::StrBytes::from_string(String::new()));
    assert_eq!(
        words(&rig.heartbeat_848(1, empty_instance).await),
        (INVALID_REQUEST, Some("InstanceId can't be empty.".into()))
    );
    let mut owned = join_848("g", "m1", &["t"]);
    owned.topic_partitions = None;
    assert_eq!(
        words(&rig.heartbeat_848(1, owned).await),
        (
            INVALID_REQUEST,
            Some("TopicPartitions must be empty when (re-)joining.".into())
        )
    );
    let mut no_timeout = join_848("g", "m1", &["t"]);
    no_timeout.rebalance_timeout_ms = -1;
    assert_eq!(
        words(&rig.heartbeat_848(1, no_timeout).await).0,
        INVALID_REQUEST
    );
    assert_eq!(
        words(&rig.heartbeat_848(1, beat_848("g", "m1", -2, None)).await),
        (INVALID_REQUEST, Some("InstanceId can't be null.".into()))
    );
    assert_eq!(
        words(&rig.heartbeat_848(1, join_848("", "m1", &["t"])).await),
        (INVALID_REQUEST, Some("GroupId can't be empty.".into()))
    );
    // A later epoch in a group that does not exist, and a leave from one.
    assert_eq!(
        words(&rig.heartbeat_848(1, beat_848("none", "m1", 5, None)).await),
        (
            GROUP_ID_NOT_FOUND,
            Some("Consumer group none not found.".into())
        )
    );
    assert_eq!(
        words(&rig.heartbeat_848(1, beat_848("none", "m1", -1, None)).await),
        (GROUP_ID_NOT_FOUND, Some("Group none not found.".into()))
    );
    rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    assert_eq!(
        rig.heartbeat_848(1, beat_848("g", "m1", 9, None))
            .await
            .error_code,
        FENCED_MEMBER_EPOCH
    );
    assert_eq!(
        rig.heartbeat_848(1, beat_848("g", "ghost", 2, None))
            .await
            .error_code,
        UNKNOWN_MEMBER_ID
    );
}

#[tokio::test]
async fn the_classic_apis_see_a_consumer_group_as_kafka_does() {
    let (rig, _) = rig().await;
    rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    // DescribeGroups does not describe it: Dead. ListGroups lists it, of type consumer.
    let d = rig.describe_groups_state("g").await;
    assert_eq!(d, "Dead");
    let listed = rig.list_groups_typed().await;
    assert_eq!(
        listed,
        [("g".to_owned(), "consumer".to_owned(), "Stable".to_owned())]
    );
    // A classic group is not a consumer group.
    rig.join(
        3,
        support::join_request("classic", "", None, &[("range", "m")]),
    )
    .await;
    let d = rig.describe_848(1, &["classic"]).await;
    assert_eq!(d[0].error_code, GROUP_ID_NOT_FOUND);
    assert_eq!(
        d[0].error_message.as_ref().unwrap().as_str(),
        "Group classic is not a consumer group."
    );
    // And it has members, so it cannot be deleted.
    assert_eq!(rig.delete_group("g").await, NON_EMPTY_GROUP);
}

#[tokio::test]
async fn offsets_are_committed_with_the_members_epoch_from_v9_and_deleted_unless_subscribed() {
    let (rig, _) = rig().await;
    rig.create(vec![topic("other", 1)], false).await;
    rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    let commit = |v: i16, epoch: i32, topic: &'static str| {
        let rig = &rig;
        async move {
            rig.commit_offsets(v, "g", "m1", epoch, &[(topic, 0, 5, None)])
                .await
                .topics[0]
                .partitions[0]
                .error_code
        }
    };
    assert_eq!(commit(9, 2, "t").await, 0);
    assert_eq!(
        commit(8, 2, "t").await,
        UNSUPPORTED_VERSION,
        "v8 has no member epoch as such"
    );
    assert_eq!(commit(9, 7, "t").await, STALE_MEMBER_EPOCH);
    assert_eq!(commit(9, 2, "other").await, 0);
    // Deleting an offset of a topic the member is subscribed to is refused; of another, it is done.
    let deleted = rig.delete_offsets("g", &[("t", 0), ("other", 0)]).await;
    assert_eq!(deleted, [SUBSCRIBED, 0]);
}

#[tokio::test]
async fn a_topic_made_after_the_member_subscribed_is_given_to_it() {
    let rig = rig_with(|c| c.consumer_assignment_interval = Duration::ZERO);
    let joined = rig.heartbeat_848(1, join_848("g", "m1", &["later"])).await;
    assert_eq!(assigned_848(&joined), Some(vec![]));
    rig.create(vec![topic("later", 2)], false).await;
    let id = rig.metadata(12, Some(&["later"]), false).await.topics[0].topic_id;
    let epoch = joined.member_epoch;
    let next = rig.heartbeat_848(1, beat_848("g", "m1", epoch, None)).await;
    assert_eq!(next.error_code, 0);
    let again = rig
        .heartbeat_848(1, beat_848("g", "m1", next.member_epoch, None))
        .await;
    let got = assigned_848(&next).or(assigned_848(&again));
    assert_eq!(got, Some(vec![(id, 0), (id, 1)]));
}
