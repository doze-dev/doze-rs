//! DescribeGroups, ListGroups, DeleteGroups and OffsetDelete through the broker.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use support::{join_request, rig_with, subscription, topic};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_GROUP_ID: i16 = 24;
const NON_EMPTY_GROUP: i16 = 68;
const GROUP_ID_NOT_FOUND: i16 = 69;
const GROUP_SUBSCRIBED_TO_TOPIC: i16 = 86;

async fn rig() -> support::Rig {
    let rig = rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO);
    rig.create(vec![topic("t", 2), topic("u", 1)], false).await;
    rig
}

/// A group `name` with one member subscribed to `topics`, synced, so that it is Stable.
async fn stable(rig: &support::Rig, name: &str, topics: &[&str]) -> String {
    let meta = subscription(topics);
    let joined = rig
        .join(3, join_request(name, "", None, &[("range", &meta)]))
        .await;
    let id = joined.member_id.to_string();
    rig.sync(3, name, &id, 1, &[(&id, "assigned")]).await;
    id
}

#[tokio::test(start_paused = true)]
async fn a_stable_group_is_described_whole_and_a_group_waiting_for_its_leader_shows_neither_protocol_nor_metadata()
 {
    let rig = rig().await;
    let id = stable(&rig, "g", &["t"]).await;
    let d = &rig.describe_groups(&["g"]).await.groups[0];
    assert_eq!(
        (
            d.error_code,
            d.group_state.as_str(),
            d.protocol_type.as_str(),
            d.protocol_data.as_str()
        ),
        (0, "Stable", "consumer", "range")
    );
    let m = &d.members[0];
    assert_eq!(m.member_id.as_str(), id);
    assert_eq!(m.member_assignment.as_ref(), b"assigned");
    assert_eq!(m.member_metadata.as_ref(), subscription(&["t"]).as_bytes());
    // A second group whose leader has not synced.
    let meta = subscription(&["t"]);
    rig.join(3, join_request("waiting", "", None, &[("range", &meta)]))
        .await;
    let d = &rig.describe_groups(&["waiting"]).await.groups[0];
    assert_eq!(d.group_state.as_str(), "CompletingRebalance");
    assert_eq!(d.protocol_data.as_str(), "", "no protocol until stable");
    assert!(d.members[0].member_metadata.is_empty());
}

#[tokio::test]
async fn a_group_nobody_has_heard_of_is_dead_and_one_with_only_offsets_is_empty() {
    let rig = rig().await;
    rig.commit_offsets(8, "old", "", -1, &[("t", 0, 5, None)])
        .await;
    let got = rig.describe_groups(&["nobody", "", "old", "a\0b"]).await;
    let states: Vec<_> = got
        .groups
        .iter()
        .map(|g| {
            (
                g.group_id.to_string(),
                g.group_state.to_string(),
                g.error_code,
            )
        })
        .collect();
    assert!(states.contains(&("nobody".into(), "Dead".into(), 0)));
    assert!(
        states.contains(&("".into(), "Dead".into(), 0)),
        "an empty id is not an error here"
    );
    assert!(states.contains(&("old".into(), "Empty".into(), 0)));
    assert!(states.contains(&("a\0b".into(), "Dead".into(), 0)));
}

#[tokio::test(start_paused = true)]
async fn groups_are_answered_in_the_order_kafka_answered_them() {
    let rig = rig().await;
    for name in ["dgo-a", "dgo-b", "dgo-c", "dgo-d"] {
        stable(&rig, name, &["t"]).await;
    }
    let order = |got: kafka_wire::messages::DescribeGroupsResponse| {
        got.groups
            .iter()
            .map(|g| g.group_id.to_string())
            .collect::<Vec<_>>()
    };
    // Recorded from Apache Kafka 4.3.1 with the same four groups and two unknown ones.
    let recorded = ["dgo-x", "dgo-y", "dgo-d", "dgo-b", "dgo-c", "dgo-a"];
    assert_eq!(
        order(
            rig.describe_groups(&["dgo-a", "dgo-b", "dgo-c", "dgo-d", "dgo-x", "dgo-y"])
                .await
        ),
        recorded
    );
    assert_eq!(
        order(
            rig.describe_groups(&["dgo-y", "dgo-x", "dgo-d", "dgo-c", "dgo-b", "dgo-a"])
                .await
        ),
        recorded
    );
    assert_eq!(
        order(rig.describe_groups(&["dgo-b", "dgo-a", "dgo-b"]).await),
        ["dgo-b", "dgo-b", "dgo-a"]
    );
}

#[tokio::test(start_paused = true)]
async fn groups_are_listed_by_id_with_their_state_and_filters_ignore_case() {
    let rig = rig().await;
    stable(&rig, "b-live", &["t"]).await;
    rig.commit_offsets(8, "a-offsets", "", -1, &[("t", 0, 1, None)])
        .await;
    let all = rig.list_groups(&[], &[]).await;
    assert_eq!(
        all,
        [
            ("a-offsets".into(), "Empty".into(), String::new()),
            ("b-live".into(), "Stable".into(), "consumer".into()),
        ]
    );
    assert_eq!(rig.list_groups(&["stable"], &[]).await.len(), 1);
    assert_eq!(
        rig.list_groups(&["EMPTY", "Stable"], &["Classic"])
            .await
            .len(),
        2
    );
    assert!(
        rig.list_groups(&[], &["consumer"]).await.is_empty(),
        "these are classic groups"
    );
    assert!(rig.list_groups(&["Dead"], &[]).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn deleting_a_group_needs_it_empty_takes_its_offsets_and_says_when_there_was_nothing() {
    let rig = rig().await;
    let id = stable(&rig, "busy", &["t"]).await;
    rig.commit_offsets(8, "idle", "", -1, &[("t", 0, 1, None)])
        .await;
    let got = rig
        .delete_groups(&["busy", "idle", "never", "", "idle"])
        .await;
    let by = |name: &str| {
        got.iter()
            .filter(|(g, _)| g == name)
            .map(|(_, c)| *c)
            .collect::<Vec<_>>()
    };
    assert_eq!(by("busy"), [NON_EMPTY_GROUP]);
    assert_eq!(by("never"), [GROUP_ID_NOT_FOUND]);
    assert_eq!(by(""), [INVALID_GROUP_ID]);
    // Deleted once, and then there was nothing.
    let idle = by("idle");
    assert!(
        idle == [0, GROUP_ID_NOT_FOUND] || idle == [GROUP_ID_NOT_FOUND, 0],
        "{idle:?}"
    );
    assert!(rig.fetch_offsets(false, "idle", None).await.is_empty());
    // After leaving, the group can go; and it is then Dead.
    rig.leave(3, "busy", &[&id]).await;
    assert_eq!(rig.delete_groups(&["busy"]).await, [("busy".into(), 0)]);
    assert_eq!(
        rig.describe_groups(&["busy"]).await.groups[0]
            .group_state
            .as_str(),
        "Dead"
    );
    // And the id can be used again as a new group.
    let again = stable(&rig, "busy", &["t"]).await;
    assert!(again.starts_with("doze-"));
}

#[tokio::test(start_paused = true)]
async fn offsets_are_deleted_unless_the_groups_members_subscribe_to_the_topic() {
    let rig = rig().await;
    stable(&rig, "g", &["t"]).await;
    // Offsets for the group are committed as a simple consumer would, by an id that is no member.
    rig.commit_offsets(8, "g", "", -1, &[]).await;
    let id = rig.describe_groups(&["g"]).await.groups[0].members[0]
        .member_id
        .to_string();
    rig.commit_offsets(8, "g", &id, 1, &[("t", 0, 5, None), ("u", 0, 6, None)])
        .await;
    let (top, parts) = rig
        .offset_delete("g", &[("t", 0), ("u", 0), ("nope", 0), ("u", 7)])
        .await;
    assert_eq!(top, 0);
    assert_eq!(
        parts,
        [
            GROUP_SUBSCRIBED_TO_TOPIC,
            0,
            UNKNOWN_TOPIC_OR_PARTITION,
            UNKNOWN_TOPIC_OR_PARTITION
        ]
    );
    let left = rig.fetch_offsets(false, "g", None).await;
    assert_eq!(
        left.iter()
            .map(|(t, p, _, _)| (t.as_str(), *p))
            .collect::<Vec<_>>(),
        [("t", 0)]
    );
}

#[tokio::test]
async fn offset_delete_of_a_group_with_nothing_is_not_found_and_an_unusable_id_is_invalid() {
    let rig = rig().await;
    assert_eq!(
        rig.offset_delete("nobody", &[("t", 0)]).await.0,
        GROUP_ID_NOT_FOUND
    );
    assert_eq!(rig.offset_delete("", &[("t", 0)]).await.0, INVALID_GROUP_ID);
    // A group with offsets and no members can have them deleted.
    rig.commit_offsets(8, "old", "", -1, &[("t", 0, 1, None)])
        .await;
    assert_eq!(rig.offset_delete("old", &[("t", 0)]).await, (0, vec![0]));
}

#[tokio::test(start_paused = true)]
async fn offset_delete_of_a_group_with_members_and_no_offsets_is_not_not_found() {
    let rig = rig().await;
    stable(&rig, "g", &["t"]).await;
    // A topic it does not subscribe to, with nothing committed: nothing to forget, and no complaint.
    assert_eq!(rig.offset_delete("g", &[("u", 0)]).await, (0, vec![0]));
}
