//! Classic consumer groups through the broker: the machine is tested alone, this checks the wiring and the wire.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_broker::Outcome;
use support::{join_request, rig_with};

const NONE: i16 = 0;
const UNKNOWN_MEMBER_ID: i16 = 25;
const INVALID_GROUP_ID: i16 = 24;
const MEMBER_ID_REQUIRED: i16 = 79;
const ILLEGAL_GENERATION: i16 = 22;
const INVALID_REQUEST: i16 = 42;

fn rig_after(delay: Duration) -> support::Rig {
    rig_with(|c| c.group_initial_rebalance_delay = delay)
}

#[tokio::test]
async fn this_broker_is_every_groups_and_transactions_coordinator_in_each_version_shape() {
    let rig = rig_after(Duration::ZERO);
    for version in 0..=3 {
        let r = rig.find_coordinator(version, &["g"], 0).await;
        assert_eq!(
            (r.error_code, r.port, r.host.as_str()),
            (NONE, 4242, "127.0.0.1")
        );
        if version > 0 {
            // Kafka puts the word NONE in a success from v1 to v3 (v0 has no message).
            let note = r.error_message.as_ref().map(|m| m.as_str());
            assert_eq!(note, Some("NONE"), "v{version}");
        }
    }
    let r = rig.find_coordinator(4, &["a", "b"], 1).await;
    let keys: Vec<_> = r
        .coordinators
        .iter()
        .map(|c| (c.key.as_str(), c.error_code, c.port))
        .collect();
    assert_eq!(keys, [("a", NONE, 4242), ("b", NONE, 4242)]);
}

#[tokio::test]
async fn a_key_type_that_has_no_coordinator_is_an_invalid_request_with_no_broker() {
    let rig = rig_after(Duration::ZERO);
    let r = rig.find_coordinator(3, &["g"], 5).await;
    assert_eq!(
        (r.error_code, r.node_id.0, r.port),
        (INVALID_REQUEST, -1, -1)
    );
    let r = rig.find_coordinator(4, &["g"], 5).await;
    assert_eq!(
        (r.coordinators[0].error_code, r.coordinators[0].node_id.0),
        (INVALID_REQUEST, -1)
    );
}

#[tokio::test]
async fn a_member_joins_syncs_heartbeats_and_leaves() {
    let rig = rig_after(Duration::ZERO);
    // Version 4: told to come back with the id it is given.
    let first = rig
        .join(4, join_request("g", "", None, &[("range", "m")]))
        .await;
    assert_eq!(first.error_code, MEMBER_ID_REQUIRED);
    let id = first.member_id.as_str().to_owned();
    assert!(id.starts_with("doze-") && id.len() == 5 + 32, "{id}");
    let joined = rig
        .join(4, join_request("g", &id, None, &[("range", "m")]))
        .await;
    assert_eq!((joined.error_code, joined.generation_id), (NONE, 1));
    assert_eq!(joined.leader.as_str(), id);
    assert_eq!(joined.protocol_name.as_ref().unwrap().as_str(), "range");
    assert_eq!(joined.members.len(), 1);
    let synced = rig.sync(4, "g", &id, 1, &[(&id, "work")]).await;
    assert_eq!(
        (synced.error_code, synced.assignment.as_ref()),
        (NONE, &b"work"[..])
    );
    assert_eq!(rig.heartbeat("g", &id, 1).await.error_code, NONE);
    assert_eq!(
        rig.heartbeat("g", &id, 2).await.error_code,
        ILLEGAL_GENERATION
    );
    let left = rig.leave(4, "g", &[&id]).await;
    assert_eq!((left.error_code, left.members[0].error_code), (NONE, NONE));
    assert_eq!(
        rig.heartbeat("g", &id, 1).await.error_code,
        UNKNOWN_MEMBER_ID
    );
}

#[tokio::test(start_paused = true)]
async fn members_starting_together_share_a_generation_and_the_follower_waits_for_the_leader() {
    let rig = rig_after(Duration::from_millis(200));
    let (a, b) = tokio::join!(
        rig.join(3, join_request("g", "", None, &[("range", "ma")])),
        rig.join(3, join_request("g", "", None, &[("range", "mb")])),
    );
    assert_eq!((a.generation_id, b.generation_id), (1, 1));
    let (leader, follower) = if a.leader == a.member_id {
        (&a, &b)
    } else {
        (&b, &a)
    };
    assert_eq!(leader.members.len(), 2);
    assert!(follower.members.is_empty());
    let (l, f) = (leader.member_id.as_str(), follower.member_id.as_str());
    let assignments = [(l, "for-leader"), (f, "for-follower")];
    let (ls, fs) = tokio::join!(
        rig.sync(3, "g", l, 1, &assignments),
        rig.sync(3, "g", f, 1, &[]),
    );
    assert_eq!(ls.assignment.as_ref(), b"for-leader");
    assert_eq!(fs.assignment.as_ref(), b"for-follower");
}

#[tokio::test(start_paused = true)]
async fn a_member_that_stops_heartbeating_is_dropped_when_its_session_runs_out() {
    let rig = rig_after(Duration::ZERO);
    let joined = rig
        .join(3, join_request("g", "", None, &[("range", "m")]))
        .await;
    let id = joined.member_id.as_str().to_owned();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(rig.heartbeat("g", &id, 1).await.error_code, NONE);
    tokio::time::sleep(Duration::from_secs(9)).await;
    assert_eq!(
        rig.heartbeat("g", &id, 1).await.error_code,
        NONE,
        "9 s since the last"
    );
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(
        rig.heartbeat("g", &id, 1).await.error_code,
        UNKNOWN_MEMBER_ID
    );
}

#[tokio::test(start_paused = true)]
async fn a_join_sent_again_ends_the_one_that_was_waiting() {
    let rig = rig_after(Duration::from_secs(5));
    let first = rig
        .join(4, join_request("g", "", None, &[("range", "m")]))
        .await;
    let id = first.member_id.as_str().to_owned();
    let waiting = rig.join_outcome(4, join_request("g", &id, None, &[("range", "m")]));
    let again = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        rig.join(4, join_request("g", &id, None, &[("range", "m")]))
            .await
    };
    let (replaced, answered) = tokio::join!(waiting, again);
    assert!(matches!(replaced, Outcome::Close), "{replaced:?}");
    assert_eq!(answered.error_code, NONE);
}

#[tokio::test]
async fn requests_to_a_group_that_does_not_exist_are_answered_as_by_an_empty_one() {
    let rig = rig_after(Duration::ZERO);
    assert_eq!(
        rig.heartbeat("nope", "m", 1).await.error_code,
        UNKNOWN_MEMBER_ID
    );
    assert_eq!(
        rig.sync(3, "nope", "m", 1, &[]).await.error_code,
        UNKNOWN_MEMBER_ID
    );
    let left = rig.leave(3, "nope", &["m"]).await;
    assert_eq!(
        (left.error_code, left.members[0].error_code),
        (NONE, UNKNOWN_MEMBER_ID)
    );
    let old = rig.leave(2, "nope", &["m"]).await;
    assert_eq!(
        old.error_code, UNKNOWN_MEMBER_ID,
        "before v3 the member's code is the request's"
    );
    // And asking did not make the group.
    let joined = rig
        .join(3, join_request("nope", "m", None, &[("range", "m")]))
        .await;
    assert_eq!(joined.error_code, UNKNOWN_MEMBER_ID);
}

#[tokio::test]
async fn an_empty_or_unusable_group_id_is_refused_by_every_group_api() {
    let rig = rig_after(Duration::ZERO);
    for bad in ["", "a\0b"] {
        assert_eq!(
            rig.join(3, join_request(bad, "", None, &[("range", "m")]))
                .await
                .error_code,
            INVALID_GROUP_ID
        );
        assert_eq!(
            rig.sync(3, bad, "m", 1, &[]).await.error_code,
            INVALID_GROUP_ID
        );
        assert_eq!(
            rig.heartbeat(bad, "m", 1).await.error_code,
            INVALID_GROUP_ID
        );
        assert_eq!(rig.leave(2, bad, &["m"]).await.error_code, INVALID_GROUP_ID);
    }
}

#[tokio::test(start_paused = true)]
async fn an_issued_id_is_used_up_by_a_join_that_is_taken_and_not_by_one_that_is_refused() {
    let rig = rig_after(Duration::ZERO);
    // A group with a member, so that a join can be refused for having nothing in common with it.
    let original = rig
        .join(3, join_request("g", "", None, &[("range", "m")]))
        .await
        .member_id
        .to_string();
    let told = rig
        .join(4, join_request("g", "", None, &[("range", "m")]))
        .await;
    assert_eq!(told.error_code, MEMBER_ID_REQUIRED);
    let id = told.member_id.as_str().to_owned();
    let refused = rig
        .join(4, join_request("g", &id, None, &[("sticky", "m")]))
        .await;
    assert_eq!(refused.error_code, 23, "nothing in common with the group");
    // Not used up: the same id is good for a join that is taken (it waits for the first member to join again).
    let (taken, rejoined) = tokio::join!(
        rig.join(4, join_request("g", &id, None, &[("range", "m")])),
        async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            rig.join(4, join_request("g", &original, None, &[("range", "m")]))
                .await
        }
    );
    assert_eq!((taken.error_code, rejoined.error_code), (NONE, NONE));
    // Used up now: after leaving, the id is no member's and nobody's to come back with.
    rig.leave(4, "g", &[&id]).await;
    let again = rig
        .join(4, join_request("g", &id, None, &[("range", "m")]))
        .await;
    assert_eq!(again.error_code, UNKNOWN_MEMBER_ID);
}

#[tokio::test(start_paused = true)]
async fn an_issued_id_is_forgotten_after_five_minutes() {
    let rig = rig_after(Duration::ZERO);
    let told = rig
        .join(4, join_request("g", "", None, &[("range", "m")]))
        .await;
    let id = told.member_id.as_str().to_owned();
    tokio::time::sleep(Duration::from_secs(299)).await;
    let early = rig
        .join(4, join_request("g", &id, None, &[("range", "m")]))
        .await;
    assert_eq!(
        early.error_code, NONE,
        "still good at four minutes fifty-nine"
    );
    rig.leave(4, "g", &[&id]).await;
    let told = rig
        .join(4, join_request("g2", "", None, &[("range", "m")]))
        .await;
    let id = told.member_id.as_str().to_owned();
    tokio::time::sleep(Duration::from_secs(301)).await;
    let late = rig
        .join(4, join_request("g2", &id, None, &[("range", "m")]))
        .await;
    assert_eq!(late.error_code, UNKNOWN_MEMBER_ID);
}

#[tokio::test]
async fn a_client_that_only_ever_asks_for_ids_leaves_no_group_behind() {
    let rig = rig_after(Duration::ZERO);
    for n in 0..300 {
        let told = rig
            .join(
                4,
                join_request(&format!("ghost{n}"), "", None, &[("range", "m")]),
            )
            .await;
        assert_eq!(told.error_code, MEMBER_ID_REQUIRED);
    }
    assert!(
        rig.list_groups(&[], &[]).await.is_empty(),
        "no group, in memory or recorded"
    );
}
