//! Every version of every group API the broker serves answers in a form that can be written. (A field set that a
//! version does not have makes the encoder refuse, and the connection is closed: a real client met that once, with
//! FindCoordinator v4.) The rig encodes every response it is given, so asking is the whole test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::offset_fetch_request::{
    OffsetFetchRequestGroup, OffsetFetchRequestTopic, OffsetFetchRequestTopics,
};
use kafka_wire::messages::{
    DeleteGroupsRequest, DescribeGroupsRequest, GroupId, HeartbeatRequest, ListGroupsRequest,
    OffsetFetchRequest, TopicName,
};
use support::{join_request, rig_with, subscription, topic};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

async fn rig() -> support::Rig {
    let rig = rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO);
    rig.create(vec![topic("t", 2)], false).await;
    rig
}

/// A stable group `name` with one member, at the generation it joined; returns the member id.
async fn stable(rig: &support::Rig, name: &str) -> String {
    let meta = subscription(&["t"]);
    let joined = rig
        .join(3, join_request(name, "", None, &[("range", &meta)]))
        .await;
    let id = joined.member_id.to_string();
    rig.sync(3, name, &id, 1, &[(&id, "work")]).await;
    id
}

#[tokio::test(start_paused = true)]
async fn finding_a_coordinator_joining_and_syncing_at_every_version() {
    let rig = rig().await;
    let meta = subscription(&["t"]);
    for v in 0..=4 {
        rig.find_coordinator(v, &["g"], 0).await;
        // A key type with no coordinator: the refusal is written too.
        rig.find_coordinator(v, &["g"], 9).await;
    }
    // The first contact (told to come back with an id, from v4), the join, and a refusal.
    for v in 0..=9 {
        let group = format!("join{v}");
        let first = rig
            .join(v, join_request(&group, "", None, &[("range", &meta)]))
            .await;
        if first.error_code == 79 {
            let id = first.member_id.to_string();
            let request = join_request(&group, &id, None, &[("range", &meta)]);
            assert_eq!(rig.join(v, request).await.error_code, 0, "join v{v}");
        }
        rig.join(v, join_request("", "", None, &[("range", &meta)]))
            .await;
    }
    for v in 0..=5 {
        let group = format!("sync{v}");
        let id = stable(&rig, &group).await;
        assert_eq!(
            rig.sync(v, &group, &id, 1, &[]).await.error_code,
            0,
            "sync v{v}"
        );
        rig.sync(v, &group, "ghost", 1, &[]).await;
    }
}

#[tokio::test(start_paused = true)]
async fn heartbeating_and_leaving_at_every_version() {
    let rig = rig().await;
    for v in 0..=4 {
        let group = format!("beat{v}");
        let id = stable(&rig, &group).await;
        for (member, generation) in [(&id[..], 1), ("ghost", 1), (&id[..], 9)] {
            let mut req = HeartbeatRequest::default();
            req.group_id = GroupId(text(&group));
            req.member_id = text(member);
            req.generation_id = generation;
            rig.ask(v, Request::Heartbeat(req)).await;
        }
    }
    for v in 0..=5 {
        let group = format!("leave{v}");
        let id = stable(&rig, &group).await;
        rig.leave(v, &group, &["ghost"]).await;
        rig.leave(v, &group, &[&id]).await;
    }
}

fn fetch_request(v: i16, ask_for_everything: bool) -> OffsetFetchRequest {
    let mut req = OffsetFetchRequest::default();
    let topics = (!ask_for_everything).then(|| vec![0, 1]);
    if v >= 8 {
        let mut g = OffsetFetchRequestGroup::default();
        g.group_id = GroupId(text("committed"));
        g.topics = topics.map(|partitions| {
            let mut t = OffsetFetchRequestTopics::default();
            t.name = TopicName(text("t"));
            t.partition_indexes = partitions;
            vec![t]
        });
        req.groups.push(g);
    } else {
        req.group_id = GroupId(text("committed"));
        req.topics = topics.map(|partitions| {
            let mut t = OffsetFetchRequestTopic::default();
            t.name = TopicName(text("t"));
            t.partition_indexes = partitions;
            vec![t]
        });
    }
    req
}

#[tokio::test(start_paused = true)]
async fn committing_fetching_and_deleting_offsets_at_every_version() {
    let rig = rig().await;
    for v in 2..=9 {
        let group = format!("commit{v}");
        let commits = [("t", 0, 5, Some("m")), ("nope", 0, 1, None)];
        let r = rig.commit_offsets(v, &group, "", -1, &commits).await;
        assert_eq!(r.topics[0].partitions[0].error_code, 0, "commit v{v}");
    }
    rig.commit_offsets(8, "committed", "", -1, &[("t", 0, 5, Some("m"))])
        .await;
    for v in 1..=9 {
        rig.ask(v, Request::OffsetFetch(fetch_request(v, false)))
            .await;
        rig.ask(v, Request::OffsetFetch(fetch_request(v, true)))
            .await;
    }
    rig.offset_delete("committed", &[("t", 0), ("t", 5)]).await;
}

#[tokio::test(start_paused = true)]
async fn describing_listing_and_deleting_groups_at_every_version() {
    let rig = rig().await;
    stable(&rig, "live").await;
    rig.commit_offsets(8, "idle", "", -1, &[("t", 0, 5, None)])
        .await;
    for v in 0..=5 {
        let mut req = DescribeGroupsRequest::default();
        req.groups = ["live", "idle", "nobody", ""]
            .iter()
            .map(|g| GroupId(text(g)))
            .collect();
        rig.ask(v, Request::DescribeGroups(req)).await;
        rig.ask(v, Request::ListGroups(ListGroupsRequest::default()))
            .await;
    }
    for v in 0..=2 {
        let mut req = DeleteGroupsRequest::default();
        req.groups_names = ["idle", "nobody", "live"]
            .iter()
            .map(|g| GroupId(text(g)))
            .collect();
        rig.ask(v, Request::DeleteGroups(req)).await;
    }
}
