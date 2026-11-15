//! Share groups (KIP-932) through the broker: the rules of docs/port/behaviours/share-groups.md that need more than one
//! machine, at the wire. The share-partition and the group are tested alone where they live (`share/partition/tests.rs`,
//! `groups/share/tests.rs`); the Java client in `interop/java/Share.java`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use support::share::{acquired, ack_request, fetch_request, heartbeat_request};
use support::{Rig, records, rig_with, topic};

const INVALID_REQUEST: i16 = 42;
const GROUP_ID_NOT_FOUND: i16 = 69;
const UNKNOWN_MEMBER_ID: i16 = 25;
const NON_EMPTY_GROUP: i16 = 68;
const INVALID_RECORD_STATE: i16 = 121;
const SHARE_SESSION_NOT_FOUND: i16 = 122;
const INVALID_SHARE_SESSION_EPOCH: i16 = 123;
const UNKNOWN_SERVER_ERROR: i16 = -1;
const UNKNOWN_TOPIC_ID: i16 = 100;
const INCONSISTENT_GROUP_PROTOCOL: i16 = 23;

const M: &str = "member-a";
const ACCEPT: i8 = 1;
const RELEASE: i8 = 2;

/// A broker whose share groups compute their assignment at every heartbeat; topic `t` has `partitions`.
async fn rig(partitions: i32) -> (Rig, Uuid) {
    let rig = rig_with(|c| c.share_assignment_interval = Some(Duration::ZERO));
    rig.create(vec![topic("t", partitions)], false).await;
    let id = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
    (rig, id)
}

/// `g` reads from the earliest record, and `M` is in it.
async fn earliest_member(rig: &Rig) {
    assert_eq!(rig.group_config("g", &[("share.auto.offset.reset", "earliest")]).await, 0);
    let joined = rig.share_join("g", M, &["t"]).await;
    assert_eq!(joined.error_code, 0, "{joined:?}");
}

#[tokio::test]
async fn a_member_joins_reads_every_partition_and_its_partitions_are_initialized_with_no_start() {
    let (rig, t) = rig(2).await;
    let joined = rig.share_join("g", M, &["t"]).await;
    assert_eq!((joined.error_code, joined.member_epoch, joined.heartbeat_interval_ms), (0, 1, 5000));
    let parts = &joined.assignment.as_ref().unwrap().topic_partitions;
    assert_eq!((parts[0].topic_id, parts[0].partitions.clone()), (t, vec![0, 1]));
    assert_eq!(joined.member_id.unwrap().as_str(), M);
    // Initialized and not yet started: no start offset and no lag (the tool prints "-").
    assert_eq!(
        rig.share_starts("g").await,
        [("t".to_owned(), 0, -1, -1), ("t".to_owned(), 1, -1, -1)]
    );
}

#[tokio::test]
async fn a_share_partition_starts_at_the_end_by_default_and_at_the_start_when_the_group_says_earliest() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(5)).await;
    rig.share_join("g", M, &["t"]).await;
    let first = rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    assert!(acquired(&first).is_empty(), "latest: the five before are not delivered");
    rig.produce(-1, "t", 0, records(3)).await;
    let next = rig.share_fetch(fetch_request("g", M, 1, &[])).await;
    assert_eq!(acquired(&next), [(0, 5, 7, 1)]);
    let (rig, t) = self::rig(1).await;
    rig.produce(-1, "t", 0, records(5)).await;
    earliest_member(&rig).await;
    let all = rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    assert_eq!(acquired(&all), [(0, 0, 4, 1)]);
    assert_eq!(all.acquisition_lock_timeout_ms, 30_000);
    assert!(!all.responses[0].partitions[0].records.as_ref().unwrap().is_empty());
}

#[tokio::test]
async fn the_share_session_is_opened_at_zero_and_each_request_must_carry_its_next_epoch() {
    let (rig, t) = rig(1).await;
    earliest_member(&rig).await;
    let top = |r: kafka_wire::messages::ShareFetchResponse| r.error_code;
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, 3, &[(t, 0, vec![])])).await), SHARE_SESSION_NOT_FOUND);
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, -1, &[])).await), SHARE_SESSION_NOT_FOUND);
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![(0, 0, vec![ACCEPT])])])).await), INVALID_REQUEST, "no acknowledgements at 0");
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await), 0);
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, 2, &[])).await), INVALID_SHARE_SESSION_EPOCH);
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, 1, &[])).await), 0);
    // ShareAcknowledge moves the same epoch, and never opens a session.
    assert_eq!(rig.share_ack_at(1, ack_request("g", M, 2, &[])).await.error_code, 0);
    assert_eq!(rig.share_ack_at(1, ack_request("g", M, 0, &[])).await.error_code, INVALID_SHARE_SESSION_EPOCH);
    assert_eq!(rig.share_ack_at(1, ack_request("g", "other", 1, &[])).await.error_code, SHARE_SESSION_NOT_FOUND);
    assert_eq!(top(rig.share_fetch(fetch_request("g", M, 3, &[])).await), 0);
}

#[tokio::test]
async fn acknowledgements_ride_on_the_next_fetch_and_the_start_moves_past_what_is_done() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(4)).await;
    earliest_member(&rig).await;
    let got = rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    assert_eq!(acquired(&got), [(0, 0, 3, 1)]);
    let next = rig
        .share_fetch(fetch_request("g", M, 1, &[(t, 0, vec![(0, 3, vec![ACCEPT, ACCEPT, RELEASE, ACCEPT])])]))
        .await;
    let p = &next.responses[0].partitions[0];
    assert_eq!((p.acknowledge_error_code, p.error_code), (0, 0));
    assert_eq!(acquired(&next), [(0, 2, 2, 2)], "the released record, its second delivery");
    // Stored: the start before the write's own advance (Kafka's lag of one write), and three of four done.
    assert_eq!(rig.share_starts("g").await, [("t".to_owned(), 0, 0, 1)]);
}

#[tokio::test]
async fn an_acknowledgement_of_what_the_member_does_not_hold_is_refused_on_its_partition() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(2)).await;
    earliest_member(&rig).await;
    rig.share_join("g", "member-b", &["t"]).await;
    rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    rig.share_fetch(fetch_request("g", "member-b", 0, &[(t, 0, vec![])])).await;
    let r = rig
        .share_ack_at(1, ack_request("g", "member-b", 1, &[(t, 0, vec![(0, 1, vec![ACCEPT])])]))
        .await;
    let p = &r.responses[0].partitions[0];
    assert_eq!(p.error_code, INVALID_RECORD_STATE);
    assert_eq!(p.error_message.as_deref(), Some("Member is not the owner of batch record"));
    // A malformed batch is INVALID_REQUEST before anything is looked at; RENEW is version 2's, with IsRenewAck.
    let r = rig
        .share_ack_at(1, ack_request("g", M, 1, &[(t, 0, vec![(1, 0, vec![ACCEPT])])]))
        .await;
    assert_eq!(r.responses[0].partitions[0].error_code, INVALID_REQUEST);
    let r = rig.share_ack_at(1, ack_request("g", M, 2, &[(t, 0, vec![(0, 1, vec![4])])])).await;
    assert_eq!(r.responses[0].partitions[0].error_code, INVALID_REQUEST);
    let unknown = rig
        .share_ack_at(1, ack_request("g", M, 3, &[(Uuid::from_u128(9), 0, vec![(0, 1, vec![ACCEPT])])]))
        .await;
    assert_eq!(unknown.responses[0].partitions[0].error_code, UNKNOWN_TOPIC_ID);
}

#[tokio::test]
async fn closing_the_session_lets_go_of_what_the_member_held_and_another_member_gets_it_again() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(3)).await;
    earliest_member(&rig).await;
    rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    let closed = rig.share_ack_at(1, ack_request("g", M, -1, &[])).await;
    assert_eq!(closed.error_code, 0);
    let b = rig.share_fetch(fetch_request("g", "member-b", 0, &[(t, 0, vec![])])).await;
    assert_eq!(acquired(&b), [(0, 0, 2, 2)]);
    assert_eq!(
        rig.share_fetch(fetch_request("g", M, 1, &[])).await.error_code,
        SHARE_SESSION_NOT_FOUND,
        "closed"
    );
}

#[tokio::test]
async fn a_connection_that_goes_closes_its_sessions_and_lets_go_of_what_they_held() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(2)).await;
    earliest_member(&rig).await;
    let on = support::at().on_connection(7);
    let head = support::head(kafka_wire::messages::ApiKey::ShareFetch, 1);
    let req = Request::ShareFetch(fetch_request("g", M, 0, &[(t, 0, vec![])]));
    match rig.broker.handle(&on, &support::caller(), &head, req).await {
        kafka_broker::Outcome::Respond(Response::ShareFetch(r)) => assert_eq!(acquired(&r), [(0, 0, 1, 1)]),
        other => panic!("{other:?}"),
    }
    rig.broker.connection_closed(7);
    let b = rig.share_fetch(fetch_request("g", "member-b", 0, &[(t, 0, vec![])])).await;
    assert_eq!(acquired(&b), [(0, 0, 1, 2)]);
}

#[tokio::test]
async fn a_request_without_a_usable_member_id_is_refused_whole() {
    let (rig, t) = rig(1).await;
    let long = "x".repeat(37);
    for member in ["", long.as_str()] {
        let r = rig.share_fetch(fetch_request("g", member, 0, &[(t, 0, vec![])])).await;
        assert_eq!(r.error_code, INVALID_REQUEST, "{member:?}");
        let r = rig.share_heartbeat(heartbeat_request("g", member, 0, Some(&["t"]))).await;
        assert_eq!((r.error_code, r.error_message), (INVALID_REQUEST, None), "the handler's error has no words");
    }
    let mut no_group = fetch_request("g", M, 0, &[]);
    no_group.group_id = None;
    assert_eq!(rig.share_fetch(no_group).await.error_code, INVALID_REQUEST);
    // A renew-only fetch (v2) must ask for nothing.
    let mut renew = fetch_request("g", M, 0, &[]);
    renew.is_renew_ack = true;
    assert_eq!(rig.share_fetch_at(2, renew).await.error_code, INVALID_REQUEST);
}

#[tokio::test]
async fn a_share_partition_never_initialized_cannot_be_fetched() {
    let (rig, t) = rig(1).await;
    let r = rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    assert_eq!(r.responses[0].partitions[0].error_code, UNKNOWN_SERVER_ERROR);
}

#[tokio::test]
async fn the_heartbeats_own_checks_are_kafkas_in_its_order_and_words() {
    let (rig, _) = rig(1).await;
    let words = |r: kafka_wire::messages::ShareGroupHeartbeatResponse| (r.error_code, r.error_message.map(|m| m.to_string()));
    assert_eq!(
        words(rig.share_heartbeat(heartbeat_request("g", M, 0, None)).await),
        (INVALID_REQUEST, Some("SubscribedTopicNames must be set in first request.".into()))
    );
    assert_eq!(
        words(rig.share_heartbeat(heartbeat_request("g", M, -2, None)).await),
        (INVALID_REQUEST, Some("MemberEpoch is invalid.".into()))
    );
    assert_eq!(
        words(rig.share_heartbeat(heartbeat_request(" ", M, 0, Some(&["t"]))).await),
        (INVALID_REQUEST, Some("GroupId can't be empty.".into()))
    );
    assert_eq!(
        words(rig.share_heartbeat(heartbeat_request("g", M, 3, None)).await),
        (GROUP_ID_NOT_FOUND, Some("Share group g not found.".into()))
    );
    assert_eq!(
        words(rig.share_heartbeat(heartbeat_request("g", M, -1, None)).await),
        (GROUP_ID_NOT_FOUND, Some("Group g not found.".into()))
    );
    rig.share_join("g", M, &["t"]).await;
    assert_eq!(
        words(rig.share_heartbeat(heartbeat_request("g", "other", 1, None)).await),
        (UNKNOWN_MEMBER_ID, Some("Member other is not a member of group g.".into()))
    );
}

#[tokio::test]
async fn a_share_group_and_the_other_kinds_refuse_each_others_requests() {
    let (rig, _) = rig(1).await;
    // A classic group (made by a commit) is not a share group.
    rig.commit_offsets(2, "classic", "", -1, &[("t", 0, 3, None)]).await;
    let r = rig.share_heartbeat(heartbeat_request("classic", M, 0, Some(&["t"]))).await;
    assert_eq!((r.error_code, r.error_message.as_deref()), (GROUP_ID_NOT_FOUND, Some("Group classic is not a share group.")));
    // A share group refuses a classic join.
    rig.share_join("g", M, &["t"]).await;
    let join = rig.join(5, support::join_request("g", "", None, &[("range", "")])).await;
    assert_eq!(join.error_code, INCONSISTENT_GROUP_PROTOCOL);
    // Listed as a share group, Stable; the classic API describes it as Dead.
    let listed = rig.list_groups(&[], &["Share"]).await;
    assert_eq!(listed, [("g".to_owned(), "Stable".to_owned(), "share".to_owned())]);
    let described = rig.describe_groups(&["g"]).await;
    assert_eq!(described.groups[0].group_state.as_str(), "Dead");
}

#[tokio::test]
async fn deleting_a_share_group_needs_it_empty_and_takes_its_share_partitions_with_it() {
    let (rig, t) = rig(1).await;
    rig.produce(-1, "t", 0, records(2)).await;
    earliest_member(&rig).await;
    rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    assert_eq!(rig.delete_groups(&["g"]).await, [("g".to_owned(), NON_EMPTY_GROUP)]);
    rig.share_heartbeat(heartbeat_request("g", M, -1, None)).await;
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
    assert_eq!((busy.error_code, busy.error_message.as_deref()), (NON_EMPTY_GROUP, Some("The group is not empty.")));
    assert!(busy.responses.is_empty());
    rig.share_heartbeat(heartbeat_request("g", M, -1, None)).await;
    let done = rig.share_alter("g", "t", &[(0, 4), (1, 0)]).await;
    assert_eq!(done.error_code, 0);
    let codes: Vec<i16> = done.responses[0].partitions.iter().map(|p| p.error_code).collect();
    assert_eq!(codes, [0, 3], "partition 1 is not there");
    assert_eq!(rig.share_starts("g").await, [("t".to_owned(), 0, 4, 2)]);
    rig.share_join("g", M, &["t"]).await;
    let got = rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    assert_eq!(acquired(&got), [(0, 4, 5, 1)]);
    // A group that is not there is made, as an empty share group.
    assert_eq!(rig.share_alter("new", "t", &[(0, 1)]).await.error_code, 0);
    let listed = rig.list_groups(&[], &["share"]).await;
    assert!(listed.iter().any(|(g, s, _)| g == "new" && s == "Empty"), "{listed:?}");
}

#[tokio::test]
async fn deleting_offsets_is_kafkas_per_topic_and_the_next_member_starts_by_the_reset_strategy() {
    let (rig, _) = rig(1).await;
    let missing = rig.share_delete_offsets("nope", &["t"]).await;
    assert_eq!((missing.error_code, missing.error_message.as_deref()), (GROUP_ID_NOT_FOUND, Some("Group nope not found.")));
    rig.share_join("g", M, &["t"]).await;
    rig.share_heartbeat(heartbeat_request("g", M, -1, None)).await;
    rig.create(vec![topic("u", 1)], false).await;
    let r = rig.share_delete_offsets("g", &["t", "u"]).await;
    let per: Vec<(String, i16, Option<String>)> = r
        .responses
        .iter()
        .map(|t| (t.topic_name.0.to_string(), t.error_code, t.error_message.as_ref().map(ToString::to_string)))
        .collect();
    assert_eq!(
        per,
        [
            ("t".to_owned(), 0, None),
            ("u".to_owned(), 3, Some("There is no offset information to delete.".to_owned()))
        ]
    );
    assert!(rig.share_starts("g").await.is_empty());
}

#[tokio::test]
async fn under_read_committed_an_aborted_transaction_is_archived_and_not_delivered() {
    let (rig, t) = rig(1).await;
    assert_eq!(
        rig.group_config("g", &[("share.isolation.level", "read_committed"), ("share.auto.offset.reset", "earliest")]).await,
        0
    );
    let tx = rig.init_txn("tx", 60_000).await.unwrap();
    rig.add_partitions("tx", tx, "t", &[0]).await;
    rig.produce_txn("tx", tx, ("t", 0), 0, 2).await;
    assert_eq!(rig.end_txn("tx", tx, false).await, 0);
    rig.produce(-1, "t", 0, records(2)).await;
    rig.share_join("g", M, &["t"]).await;
    let got = rig.share_fetch(fetch_request("g", M, 0, &[(t, 0, vec![])])).await;
    // Offsets 0-1 the aborted records, 2 the abort marker (acquired, to come back as a gap), 3-4 the plain records.
    assert_eq!(acquired(&got), [(0, 2, 4, 1)]);
}

#[tokio::test]
async fn group_configs_are_described_with_their_source_and_refused_in_kafkas_words() {
    let (rig, _) = rig(1).await;
    assert_eq!(rig.group_config("g", &[("share.record.lock.duration.ms", "15000")]).await, 0);
    let described = rig.describe_configs(4, 32, "g", None, true, false).await;
    assert_eq!(described.error_code, 0);
    let of = |name: &str| {
        let c = described.configs.iter().find(|c| c.name.as_str() == name).unwrap();
        (c.value.as_ref().unwrap().to_string(), c.config_source)
    };
    assert_eq!(of("share.record.lock.duration.ms"), ("15000".to_owned(), 8), "GROUP_CONFIG");
    assert_eq!(of("share.auto.offset.reset"), ("latest".to_owned(), 5), "DEFAULT_CONFIG");
    assert_eq!(described.configs.len(), 9);
    assert_eq!(rig.group_config("g", &[("share.delivery.count.limit", "11")]).await, 40);
    assert_eq!(rig.group_config("g", &[("nope", "1")]).await, 40);
}
