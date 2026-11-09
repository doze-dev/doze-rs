//! DescribeGroups and ConsumerGroupDescribe show where each member connected from.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{ConsumerGroupDescribeRequest, DescribeGroupsRequest, GroupId};
use support::{join_848, join_request, rig_with, topic};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

#[tokio::test]
async fn a_member_is_shown_with_the_address_it_connected_from() {
    let rig = rig_with(|c| {
        c.group_initial_rebalance_delay = Duration::ZERO;
        c.consumer_assignment_interval = Duration::ZERO;
    });
    rig.create(vec![topic("t", 2)], false).await;
    // A classic member from one address, and a member of the new protocol, in another group, from an IPv6 one.
    let joined = match rig
        .ask_from(
            "10.1.2.3",
            3,
            Request::JoinGroup(join_request("classic", "", None, &[("range", "m")])),
        )
        .await
    {
        Response::JoinGroup(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(joined.error_code, 0);
    let mut req = DescribeGroupsRequest::default();
    req.groups = vec![GroupId(text("classic"))];
    let Response::DescribeGroups(r) = rig.ask(5, Request::DescribeGroups(req)).await else {
        panic!("not DescribeGroups")
    };
    assert_eq!(r.groups[0].members[0].client_host.as_str(), "/10.1.2.3");
    let beat = rig
        .ask_from(
            "::1",
            1,
            Request::ConsumerGroupHeartbeat(join_848("new", "m1", &["t"])),
        )
        .await;
    assert!(matches!(beat, Response::ConsumerGroupHeartbeat(ref b) if b.error_code == 0));
    let mut req = ConsumerGroupDescribeRequest::default();
    req.group_ids = vec![GroupId(text("new"))];
    let Response::ConsumerGroupDescribe(r) = rig.ask(0, Request::ConsumerGroupDescribe(req)).await
    else {
        panic!("not ConsumerGroupDescribe")
    };
    assert_eq!(r.groups[0].members[0].client_host.as_str(), "/::1");
}
