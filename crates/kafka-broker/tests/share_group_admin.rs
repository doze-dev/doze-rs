//! Share groups (KIP-932) through the broker, the group side: the heartbeat's checks, a share group and the other kinds,
//! deleting a group, its offsets (AlterShareGroupOffsets, DeleteShareGroupOffsets) and its configs. The fetching side is
//! in `share_groups.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::Uuid;
use support::share::{acquired, fetch_request, heartbeat_request};
use support::{Rig, records, rig_with, topic};

const INVALID_REQUEST: i16 = 42;
const GROUP_ID_NOT_FOUND: i16 = 69;
const UNKNOWN_MEMBER_ID: i16 = 25;
const NON_EMPTY_GROUP: i16 = 68;
const INCONSISTENT_GROUP_PROTOCOL: i16 = 23;

const M: &str = "member-a";

/// A broker whose share groups compute their assignment at every heartbeat; topic `t` has `partitions`.
async fn rig(partitions: i32) -> (Rig, Uuid) {
    let rig = rig_with(|c| c.share_assignment_interval = Some(Duration::ZERO));
    rig.create(vec![topic("t", partitions)], false).await;
    let id = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
    (rig, id)
}

/// `g` reads from the earliest record, and `M` is in it.
async fn earliest_member(rig: &Rig) {
    assert_eq!(
        rig.group_config("g", &[("share.auto.offset.reset", "earliest")])
            .await,
        0
    );
    let joined = rig.share_join("g", M, &["t"]).await;
    assert_eq!(joined.error_code, 0, "{joined:?}");
}

#[tokio::test]
async fn the_heartbeats_own_checks_are_kafkas_in_its_order_and_words() {
    let (rig, _) = rig(1).await;
    let words = |r: kafka_wire::messages::ShareGroupHeartbeatResponse| {
        (r.error_code, r.error_message.map(|m| m.to_string()))
    };
    assert_eq!(
        words(
            rig.share_heartbeat(heartbeat_request("g", M, 0, None))
                .await
        ),
        (
            INVALID_REQUEST,
            Some("SubscribedTopicNames must be set in first request.".into())
        )
    );
    assert_eq!(
        words(
            rig.share_heartbeat(heartbeat_request("g", M, -2, None))
                .await
        ),
        (INVALID_REQUEST, Some("MemberEpoch is invalid.".into()))
    );
    assert_eq!(
        words(
            rig.share_heartbeat(heartbeat_request(" ", M, 0, Some(&["t"])))
                .await
        ),
        (INVALID_REQUEST, Some("GroupId can't be empty.".into()))
    );
    assert_eq!(
        words(
            rig.share_heartbeat(heartbeat_request("g", M, 3, None))
                .await
        ),
        (GROUP_ID_NOT_FOUND, Some("Share group g not found.".into()))
    );
    assert_eq!(
        words(
            rig.share_heartbeat(heartbeat_request("g", M, -1, None))
                .await
        ),
        (GROUP_ID_NOT_FOUND, Some("Group g not found.".into()))
    );
    rig.share_join("g", M, &["t"]).await;
    assert_eq!(
        words(
            rig.share_heartbeat(heartbeat_request("g", "other", 1, None))
                .await
        ),
        (
            UNKNOWN_MEMBER_ID,
            Some("Member other is not a member of group g.".into())
        )
    );
}

#[tokio::test]
async fn a_share_group_and_the_other_kinds_refuse_each_others_requests() {
    let (rig, _) = rig(1).await;
    // A classic group (made by a commit) is not a share group.
    rig.commit_offsets(2, "classic", "", -1, &[("t", 0, 3, None)])
        .await;
    let r = rig
        .share_heartbeat(heartbeat_request("classic", M, 0, Some(&["t"])))
        .await;
    assert_eq!(
        (r.error_code, r.error_message.as_deref()),
        (
            GROUP_ID_NOT_FOUND,
            Some("Group classic is not a share group.")
        )
    );
    // A share group refuses a classic join.
    rig.share_join("g", M, &["t"]).await;
    let join = rig
        .join(5, support::join_request("g", "", None, &[("range", "")]))
        .await;
    assert_eq!(join.error_code, INCONSISTENT_GROUP_PROTOCOL);
    // Listed as a share group, Stable; the classic API describes it as Dead.
    let listed = rig.list_groups(&[], &["Share"]).await;
    assert_eq!(
        listed,
        [("g".to_owned(), "Stable".to_owned(), "share".to_owned())]
    );
    let described = rig.describe_groups(&["g"]).await;
    assert_eq!(described.groups[0].group_state.as_str(), "Dead");
}

#[tokio::test]
async fn deleting_a_share_group_needs_it_empty_and_takes_its_share_partitions_with_it() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(2)).await;
    earliest_member(&rig).await;
    rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])]))
        .await;
    assert_eq!(
        rig.delete_groups(&["g"]).await,
        [("g".to_owned(), NON_EMPTY_GROUP)]
    );
    rig.share_heartbeat(heartbeat_request("g", M, -1, None))
        .await;
    assert_eq!(rig.delete_groups(&["g"]).await, [("g".to_owned(), 0)]);
    assert!(rig.share_starts("g").await.is_empty());
    assert!(rig.list_groups(&[], &["share"]).await.is_empty());
}

#[tokio::test]
async fn offsets_are_reset_only_while_the_group_is_empty_and_a_group_not_there_is_made() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(6)).await;
    earliest_member(&rig).await;
    let busy = rig.share_alter("g", "t", &[(0, 4)]).await;
    assert_eq!(
        (busy.error_code, busy.error_message.as_deref()),
        (NON_EMPTY_GROUP, Some("The group is not empty."))
    );
    assert!(busy.responses.is_empty());
    rig.share_heartbeat(heartbeat_request("g", M, -1, None))
        .await;
    let done = rig.share_alter("g", "t", &[(0, 4), (1, 0)]).await;
    assert_eq!(done.error_code, 0);
    let codes: Vec<i16> = done.responses[0]
        .partitions
        .iter()
        .map(|p| p.error_code)
        .collect();
    assert_eq!(codes, [0, 3], "partition 1 is not there");
    assert_eq!(rig.share_starts("g").await, [("t".to_owned(), 0, 4, 2)]);
    rig.share_join("g", M, &["t"]).await;
    let got = rig
        .share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])]))
        .await;
    assert_eq!(acquired(&got), [(0, 4, 5, 1)]);
    // A group that is not there is made, as an empty share group.
    assert_eq!(rig.share_alter("new", "t", &[(0, 1)]).await.error_code, 0);
    let listed = rig.list_groups(&[], &["share"]).await;
    assert!(
        listed.iter().any(|(g, s, _)| g == "new" && s == "Empty"),
        "{listed:?}"
    );
}

#[tokio::test]
async fn deleting_offsets_is_kafkas_per_topic_and_the_next_member_starts_by_the_reset_strategy() {
    let (rig, _) = rig(1).await;
    let missing = rig.share_delete_offsets("nope", &["t"]).await;
    assert_eq!(
        (missing.error_code, missing.error_message.as_deref()),
        (GROUP_ID_NOT_FOUND, Some("Group nope not found."))
    );
    rig.share_join("g", M, &["t"]).await;
    rig.share_heartbeat(heartbeat_request("g", M, -1, None))
        .await;
    rig.create(vec![topic("u", 1)], false).await;
    let r = rig.share_delete_offsets("g", &["t", "u"]).await;
    let per: Vec<(String, i16, Option<String>)> = r
        .responses
        .iter()
        .map(|t| {
            (
                t.topic_name.0.to_string(),
                t.error_code,
                t.error_message.as_ref().map(ToString::to_string),
            )
        })
        .collect();
    assert_eq!(
        per,
        [
            ("t".to_owned(), 0, None),
            (
                "u".to_owned(),
                3,
                Some("There is no offset information to delete.".to_owned())
            )
        ]
    );
    assert!(rig.share_starts("g").await.is_empty());
}

#[tokio::test]
async fn group_configs_are_described_with_their_source_and_refused_in_kafkas_words() {
    let (rig, _) = rig(1).await;
    assert_eq!(
        rig.group_config("g", &[("share.record.lock.duration.ms", "15000")])
            .await,
        0
    );
    let described = rig.describe_configs(4, 32, "g", None, true, false).await;
    assert_eq!(described.error_code, 0);
    let of = |name: &str| {
        let c = described
            .configs
            .iter()
            .find(|c| c.name.as_str() == name)
            .unwrap();
        (c.value.as_ref().unwrap().to_string(), c.config_source)
    };
    assert_eq!(
        of("share.record.lock.duration.ms"),
        ("15000".to_owned(), 8),
        "GROUP_CONFIG"
    );
    assert_eq!(
        of("share.auto.offset.reset"),
        ("latest".to_owned(), 5),
        "DEFAULT_CONFIG"
    );
    assert_eq!(described.configs.len(), 9);
    assert_eq!(
        rig.group_config("g", &[("share.delivery.count.limit", "11")])
            .await,
        40
    );
    assert_eq!(rig.group_config("g", &[("nope", "1")]).await, 40);
}
