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
async fn every_served_version_of_every_group_api_answers_in_a_form_that_can_be_written() {
    let rig = rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO);
    rig.create(vec![topic("t", 2)], false).await;
    let meta = subscription(&["t"]);

    for v in 0..=4 {
        rig.find_coordinator(v, &["g"], 0).await;
        rig.find_coordinator(v, &["g"], 9).await; // a key type with no coordinator: the refusal is written too
    }
    // Joining: the first contact (told to come back with an id, from v4), the join, and a refusal.
    for v in 0..=9 {
        let group = format!("join{v}");
        let first = rig
            .join(v, join_request(&group, "", None, &[("range", &meta)]))
            .await;
        if first.error_code == 79 {
            let id = first.member_id.to_string();
            let joined = rig
                .join(v, join_request(&group, &id, None, &[("range", &meta)]))
                .await;
            assert_eq!(joined.error_code, 0, "join v{v}");
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
    for v in 2..=9 {
        let group = format!("commit{v}");
        let r = rig
            .commit_offsets(
                v,
                &group,
                "",
                -1,
                &[("t", 0, 5, Some("m")), ("nope", 0, 1, None)],
            )
            .await;
        assert_eq!(r.topics[0].partitions[0].error_code, 0, "commit v{v}");
    }
    for v in 1..=9 {
        let mut req = OffsetFetchRequest::default();
        if v >= 8 {
            let mut g = OffsetFetchRequestGroup::default();
            g.group_id = GroupId(text("commit5"));
            let mut t = OffsetFetchRequestTopics::default();
            t.name = TopicName(text("t"));
            t.partition_indexes = vec![0, 1];
            g.topics = Some(vec![t]);
            req.groups.push(g);
        } else {
            req.group_id = GroupId(text("commit5"));
            let mut t = OffsetFetchRequestTopic::default();
            t.name = TopicName(text("t"));
            t.partition_indexes = vec![0, 1];
            req.topics = Some(vec![t]);
        }
        rig.ask(v, Request::OffsetFetch(req)).await;
        // And the form that asks for everything.
        let mut all = OffsetFetchRequest::default();
        if v >= 8 {
            let mut g = OffsetFetchRequestGroup::default();
            g.group_id = GroupId(text("commit5"));
            all.groups.push(g);
        } else {
            all.group_id = GroupId(text("commit5"));
        }
        rig.ask(v, Request::OffsetFetch(all)).await;
    }
    for v in 0..=5 {
        let mut req = DescribeGroupsRequest::default();
        req.groups = ["sync5", "nobody", ""]
            .iter()
            .map(|g| GroupId(text(g)))
            .collect();
        rig.ask(v, Request::DescribeGroups(req)).await;
        rig.ask(v, Request::ListGroups(ListGroupsRequest::default()))
            .await;
    }
    rig.offset_delete("commit5", &[("t", 0), ("t", 5)]).await;
    for v in 0..=2 {
        let mut req = DeleteGroupsRequest::default();
        req.groups_names = ["commit2", "nobody", "sync0"]
            .iter()
            .map(|g| GroupId(text(g)))
            .collect();
        rig.ask(v, Request::DeleteGroups(req)).await;
    }
}
