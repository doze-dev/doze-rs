//! Authorization of FindCoordinator and the classic group APIs: JoinGroup, SyncGroup, Heartbeat, LeaveGroup,
//! DescribeGroups, ListGroups, DeleteGroups. No row is recorded: each test cites the Kafka 4.3.1 source it is
//! written from (`core/src/main/scala/kafka/server/KafkaApis.scala`, "KA" below).
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

use gx::{
    DELETE, DESCRIBE, GROUP, GROUP_AUTH, READ, TXN_AUTH, TXN_ID, allow, creation, deny, need, world,
};
use support::acls::{admin, grant, user};
use support::rig_with;

// ---- FindCoordinator (KA:1244-1250: DESCRIBE on the group, or on the transactional id; before any lookup)

#[tokio::test]
async fn find_coordinator_for_a_group_needs_describe_on_it() {
    five_ways!(&[need(GROUP, "grp", DESCRIBE, GROUP_AUTH)], |rig, who| {
        calls::find_coordinator(rig, who, 4, &["grp"], 0)
            .await
            .coordinators[0]
            .error_code
    });
}

#[tokio::test]
async fn find_coordinator_for_a_transaction_needs_describe_on_the_transactional_id() {
    five_ways!(&[need(TXN_ID, "tid", DESCRIBE, TXN_AUTH)], |rig, who| {
        calls::find_coordinator(rig, who, 4, &["tid"], 1)
            .await
            .coordinators[0]
            .error_code
    });
}

#[tokio::test]
async fn find_coordinator_up_to_v3_refuses_at_the_top_with_no_broker() {
    let rig = world();
    for version in 0..=3 {
        let r = calls::find_coordinator(&rig, &user("bob"), version, &["grp"], 0).await;
        assert_eq!(
            (r.error_code, r.node_id.0, r.port, r.host.as_str()),
            (30, -1, -1, ""),
            "v{version}"
        );
        if version > 0 {
            // `prepareOldResponse` writes the error's own message (FindCoordinatorResponse.java:139-147).
            let said = r.error_message.as_ref().map(|m| m.as_str());
            assert_eq!(said, Some("Group authorization failed."), "v{version}");
        }
    }
    let r = calls::find_coordinator(&rig, &user("bob"), 3, &["tid"], 1).await;
    assert_eq!((r.error_code, r.node_id.0, r.port), (53, -1, -1));
    let said = r.error_message.as_ref().map(|m| m.as_str());
    assert_eq!(said, Some("Transactional Id authorization failed."));
}

#[tokio::test]
async fn find_coordinator_v4_refuses_per_key_and_serves_the_rest() {
    let rig = world();
    grant(&rig, vec![allow(GROUP, "ok", "User:bob", DESCRIBE)]).await;
    let r = calls::find_coordinator(&rig, &user("bob"), 4, &["no", "ok", "no2"], 0).await;
    let seen: Vec<_> = r
        .coordinators
        .iter()
        .map(|c| (c.key.as_str(), c.error_code, c.node_id.0, c.port))
        .collect();
    assert_eq!(
        seen,
        [("no", 30, -1, -1), ("ok", 0, 1, 4242), ("no2", 30, -1, -1)]
    );
    assert_eq!(r.coordinators[0].host.as_str(), "");
    // The entry says nothing more (the field's default is the empty text).
    let said = r.coordinators[0].error_message.as_ref().map(|m| m.as_str());
    assert_eq!(said, Some(""));
}

#[tokio::test]
async fn find_coordinator_asks_nothing_for_a_key_type_that_is_no_coordinators() {
    // KA:1244-1250 checks only key types 0 and 1: another is answered INVALID_REQUEST to everyone.
    let rig = world();
    let r = calls::find_coordinator(&rig, &user("bob"), 4, &["g"], 5).await;
    assert_eq!(r.coordinators[0].error_code, 42);
}

#[tokio::test]
async fn describe_is_implied_by_read_and_a_deny_of_read_does_not_take_it_away() {
    let rig = world();
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(GROUP, "h", "User:carol", DESCRIBE),
            deny(GROUP, "h", "User:carol", READ),
        ],
    )
    .await;
    let code = |who: &str, key: &'static str| {
        let (rig, who) = (&rig, user(who));
        async move {
            calls::find_coordinator(rig, &who, 4, &[key], 0)
                .await
                .coordinators[0]
                .error_code
        }
    };
    assert_eq!(code("alice", "g").await, 0);
    assert_eq!(code("carol", "h").await, 0);
}

// ---- JoinGroup, SyncGroup, Heartbeat, LeaveGroup (KA:1376, 1400, 1466, 1486: READ on the group, first)

/// Joins as `who` and, if taken in, has the super user make the member leave again.
async fn join_and_leave(rig: &support::Rig, who: &kafka_broker::Caller) -> i16 {
    let r = calls::join(rig, who, 3, "grp", "").await;
    if r.error_code == 0 {
        calls::leave(rig, &admin(), 3, "grp", r.member_id.as_str()).await;
    }
    r.error_code
}

#[tokio::test]
async fn join_group_needs_read_on_the_group() {
    five_ways!(&[need(GROUP, "grp", READ, GROUP_AUTH)], |rig, who| {
        join_and_leave(rig, who).await
    });
}

#[tokio::test]
async fn a_member_with_read_is_taken_in_and_one_without_is_not_made_a_member() {
    let rig = world();
    grant(&rig, vec![allow(GROUP, "grp", "User:alice", READ)]).await;
    let r = calls::join(&rig, &user("alice"), 3, "grp", "").await;
    assert_eq!((r.error_code, r.generation_id), (0, 1));
    let r = calls::join(&rig, &user("bob"), 3, "grp", "").await;
    assert_eq!(r.error_code, 30);
    let described = calls::describe_groups(&rig, &admin(), 5, &["grp"], false).await;
    assert_eq!(described.groups[0].members.len(), 1, "bob joined nothing");
}

#[tokio::test]
async fn join_group_refusal_has_the_shape_of_each_version() {
    // JoinGroupRequest.getErrorResponse: generation -1, empty ids, no members; the protocol name is the empty text
    // before v7 and null from v7.
    let rig = world();
    for version in [0_i16, 3, 5, 6, 7, 9] {
        let r = calls::join(&rig, &user("bob"), version, "grp", "m").await;
        assert_eq!(r.error_code, 30, "v{version}");
        assert_eq!(r.generation_id, -1, "v{version}");
        assert_eq!(r.member_id.as_str(), "", "v{version}");
        assert_eq!(r.leader.as_str(), "", "v{version}");
        assert!(r.members.is_empty(), "v{version}");
        let name = r.protocol_name.as_ref().map(|n| n.as_str());
        assert_eq!(
            name,
            if version >= 7 { None } else { Some("") },
            "v{version}"
        );
    }
}

#[tokio::test]
async fn authorization_comes_before_the_group_id_is_judged() {
    // KA:1376-1398 authorizes the id as sent, the empty one too (no ACL can name it: CreateAcls refuses an empty name,
    // so only a super user or the default passes); INVALID_GROUP_ID is the coordinator's.
    let rig = world();
    assert_eq!(
        calls::join(&rig, &user("bob"), 3, "", "").await.error_code,
        30
    );
    assert_eq!(calls::join(&rig, &admin(), 3, "", "").await.error_code, 24);
    let open = support::rig_with(gx::open);
    assert_eq!(
        calls::join(&open, &user("bob"), 3, "", "").await.error_code,
        24
    );
}

#[tokio::test]
async fn sync_group_needs_read_on_the_group() {
    five_ways!(&[need(GROUP, "grp", READ, GROUP_AUTH)], |rig, who| {
        calls::sync(rig, who, 4, "grp", "m", true).await.error_code
    });
}

#[tokio::test]
async fn sync_group_looks_for_the_protocol_before_it_authorizes() {
    // KA:1406-1411: from v5 a SyncGroup with no protocol type or name is INCONSISTENT_GROUP_PROTOCOL (23) before the
    // group is authorized; before v5 nothing is asked of it.
    let rig = world();
    let bob = user("bob");
    assert_eq!(
        calls::sync(&rig, &bob, 5, "grp", "m", false)
            .await
            .error_code,
        23
    );
    assert_eq!(
        calls::sync(&rig, &bob, 5, "grp", "m", true)
            .await
            .error_code,
        30
    );
    assert_eq!(
        calls::sync(&rig, &bob, 4, "grp", "m", false)
            .await
            .error_code,
        30
    );
}

#[tokio::test]
async fn heartbeat_needs_read_on_the_group() {
    five_ways!(&[need(GROUP, "grp", READ, GROUP_AUTH)], |rig, who| {
        calls::heartbeat(rig, who, "grp", "m").await.error_code
    });
}

#[tokio::test]
async fn leave_group_needs_read_on_the_group() {
    five_ways!(&[need(GROUP, "grp", READ, GROUP_AUTH)], |rig, who| {
        let top = calls::leave(rig, who, 3, "grp", "m").await;
        // A refusal is at the top; an answer to the member is in the list.
        let member = top.members.first().map_or(0, |m| m.error_code);
        if top.error_code == 0 {
            member
        } else {
            top.error_code
        }
    });
}

#[tokio::test]
async fn leave_group_refusal_is_at_the_top_in_every_version() {
    // LeaveGroupRequest.getErrorResponse sets the top-level code and no members (KA:1486-1504).
    let rig = world();
    for version in 0..=5 {
        let r = calls::leave(&rig, &user("bob"), version, "grp", "m").await;
        assert_eq!(r.error_code, 30, "v{version}");
        assert!(r.members.is_empty(), "v{version}");
    }
}

// ---- DescribeGroups (KA:1302-1348)

#[tokio::test]
async fn describe_groups_needs_describe_on_each_group() {
    five_ways!(&[need(GROUP, "grp", DESCRIBE, GROUP_AUTH)], |rig, who| {
        calls::describe_groups(rig, who, 5, &["grp"], false)
            .await
            .groups[0]
            .error_code
    });
}

#[tokio::test]
async fn describe_groups_puts_the_refused_first_and_serves_the_rest() {
    let rig = world();
    grant(&rig, vec![allow(GROUP, "b", "User:bob", DESCRIBE)]).await;
    let r = calls::describe_groups(&rig, &user("bob"), 5, &["a", "b", "c"], false).await;
    let seen: Vec<_> = r
        .groups
        .iter()
        .map(|g| (g.group_id.as_str(), g.error_code, g.group_state.as_str()))
        .collect();
    // The refused in the order asked (KA:1312-1316), then what the coordinator said (KA:1337-1343).
    assert_eq!(seen, [("a", 30, ""), ("c", 30, ""), ("b", 0, "Dead")]);
    let refused = &r.groups[0];
    assert!(refused.protocol_type.is_empty() && refused.protocol_data.is_empty());
    assert!(refused.members.is_empty());
    assert_eq!(refused.authorized_operations, i32::MIN);
}

#[tokio::test]
async fn describe_groups_says_which_operations_the_caller_has() {
    // AuthHelper.authorizedOperations (AH:62-76): one bit per supported operation of a group (READ 3, DELETE 6,
    // DESCRIBE 8, DESCRIBE_CONFIGS 10, ALTER_CONFIGS 11) that is allowed; ALL is never a bit. KA:1326-1335: only
    // from v3, only when asked, only for a group that is not an error.
    let rig = world();
    grant(
        &rig,
        vec![
            allow(GROUP, "g", "User:alice", READ),
            allow(GROUP, "g", "User:bob", gx::ALL),
            allow(GROUP, "g", "User:carol", DELETE),
            deny(GROUP, "g", "User:dave", DESCRIBE),
            allow(GROUP, "g", "User:dave", READ),
            allow(GROUP, "g", "User:erin", DESCRIBE),
        ],
    )
    .await;
    let bits = |who: &str, version: i16, asked: bool| {
        let (rig, who) = (&rig, user(who));
        async move {
            calls::describe_groups(rig, &who, version, &["g"], asked)
                .await
                .groups[0]
                .authorized_operations
        }
    };
    // READ allows READ and implies DESCRIBE.
    assert_eq!(bits("alice", 5, true).await, (1 << 3) | (1 << 8));
    // ALL allows all five; the super user too, whatever is denied it.
    assert_eq!(bits("bob", 5, true).await, 8 + 64 + 256 + 1024 + 2048);
    assert_eq!(bits("admin", 5, true).await, 8 + 64 + 256 + 1024 + 2048);
    // DELETE implies DESCRIBE.
    assert_eq!(bits("carol", 3, true).await, (1 << 6) | (1 << 8));
    // A DENY of DESCRIBE takes the DESCRIBE bit and the group (the group itself is then refused).
    assert_eq!(
        calls::describe_groups(&rig, &user("dave"), 5, &["g"], true)
            .await
            .groups[0]
            .error_code,
        30
    );
    assert_eq!(bits("erin", 5, true).await, 1 << 8);
    // Not asked, or before v3: the schema's default.
    assert_eq!(bits("alice", 5, false).await, i32::MIN);
    assert_eq!(bits("alice", 2, true).await, i32::MIN);
}

// ---- ListGroups (KA:1350-1374)

async fn two_groups_with_offsets(rig: &support::Rig) {
    gx::make_topic(rig, "t", 1).await;
    for g in ["g1", "g2"] {
        let r = calls::commit(rig, &admin(), 8, g, &[("t", 0, 1)]).await;
        assert_eq!(calls::first_commit_code(&r), 0);
    }
}

#[tokio::test]
async fn list_groups_shows_only_the_groups_the_caller_may_describe() {
    let rig = world();
    two_groups_with_offsets(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g1", "User:alice", DESCRIBE),
            allow(GROUP, "g2", "User:carol", READ),
            deny(GROUP, "g2", "User:carol", DESCRIBE),
        ],
    )
    .await;
    assert_eq!(calls::list_groups(&rig, &user("alice")).await, ["g1"]);
    assert!(calls::list_groups(&rig, &user("bob")).await.is_empty());
    assert!(calls::list_groups(&rig, &user("carol")).await.is_empty());
    // The super user sees both, whatever is denied it.
    assert_eq!(calls::list_groups(&rig, &admin()).await, ["g1", "g2"]);
}

#[tokio::test]
async fn describe_on_the_cluster_lists_every_group_and_a_group_deny_does_not_narrow_it() {
    // KA:1352, 1361-1363: with DESCRIBE on the cluster all groups are returned, with no per-group check.
    let rig = world();
    two_groups_with_offsets(&rig).await;
    grant(
        &rig,
        vec![
            allow(gx::CLUSTER, "kafka-cluster", "User:dave", DESCRIBE),
            deny(GROUP, "g1", "User:dave", DESCRIBE),
        ],
    )
    .await;
    assert_eq!(calls::list_groups(&rig, &user("dave")).await, ["g1", "g2"]);
}

#[tokio::test]
async fn list_groups_by_any_user_prefix_and_the_default() {
    let rig = world();
    two_groups_with_offsets(&rig).await;
    grant(
        &rig,
        vec![
            creation((GROUP, "g", gx::PREFIXED), "User:erin", DESCRIBE, gx::ALLOW),
            allow(GROUP, "g2", "User:*", DESCRIBE),
        ],
    )
    .await;
    assert_eq!(calls::list_groups(&rig, &user("erin")).await, ["g1", "g2"]);
    assert_eq!(calls::list_groups(&rig, &user("zed")).await, ["g2"]);
    // A broker whose default allows shows everything while no ACL covers a group.
    let open = rig_with(gx::open);
    two_groups_with_offsets(&open).await;
    assert_eq!(calls::list_groups(&open, &user("zed")).await, ["g1", "g2"]);
    // (With nothing covering the cluster the default lets everyone describe it, and so list every group; once an ACL
    // covers the cluster, only the groups' own ACLs decide.)
    grant(&open, vec![allow(GROUP, "g1", "User:alice", DESCRIBE)]).await;
    assert_eq!(calls::list_groups(&open, &user("zed")).await, ["g1", "g2"]);
    grant(
        &open,
        vec![allow(gx::CLUSTER, "kafka-cluster", "User:alice", DESCRIBE)],
    )
    .await;
    assert_eq!(calls::list_groups(&open, &user("zed")).await, ["g2"]);
}

// ---- DeleteGroups (KA:1428-1464: DELETE on each group, the request's names de-duplicated first)

#[tokio::test]
async fn delete_groups_needs_delete_on_each_group() {
    five_ways!(&[need(GROUP, "grp", DELETE, GROUP_AUTH)], |rig, who| {
        calls::delete_groups(rig, who, &["grp"]).await[0].1
    });
}

#[tokio::test]
async fn delete_groups_serves_the_allowed_first_then_the_refused_once_each() {
    let rig = world();
    two_groups_with_offsets(&rig).await;
    grant(&rig, vec![allow(GROUP, "g2", "User:alice", DELETE)]).await;
    let r = calls::delete_groups(&rig, &user("alice"), &["g1", "g2", "g1"]).await;
    // g2 is deleted (the coordinator's answer comes first, KA:1456-1460), then g1 is refused, once.
    assert_eq!(r, [("g2".to_owned(), 0), ("g1".to_owned(), 30)]);
    // The refused group still has its offsets: nothing was deleted that was not allowed.
    assert_eq!(calls::list_groups(&rig, &admin()).await, ["g1"]);
}

#[tokio::test]
async fn delete_needs_delete_and_describe_or_read_do_not_do() {
    let rig = world();
    two_groups_with_offsets(&rig).await;
    grant(
        &rig,
        vec![
            allow(GROUP, "g1", "User:alice", READ),
            allow(GROUP, "g1", "User:alice", DESCRIBE),
        ],
    )
    .await;
    assert_eq!(
        calls::delete_groups(&rig, &user("alice"), &["g1"]).await[0].1,
        30
    );
}
