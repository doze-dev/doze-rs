//! The Story of a group over a real broker in the same process, with real members: a classic group that joins and leaves, a
//! consumer-protocol group whose topic grows, a group that expires and comes back until it flaps, a caller that may not
//! describe the group, and group ids that are hostile input.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_groups;
mod support_write;

use std::time::Duration;

use axum::http::StatusCode;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::leave_group_request::MemberIdentity;
use kafka_wire::messages::{GroupId, LeaveGroupRequest};
use support::get;
use support_groups::*;
use support_write::{World, fragment_get, page, router_of};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A broker whose groups form at once (no initial wait) with `orders` made.
async fn instant(name: &str) -> World {
    with_orders(name, |c| c.group_initial_rebalance_delay = Duration::ZERO).await
}

/// A classic member joins with a reason (JoinGroup v5 says why) and is synced: the member id.
async fn join_with_reason(w: &World, group: &str, reason: &str) -> String {
    let mut req = join_request(group, "", "range", &["orders"]);
    req.reason = Some(text(reason));
    let Response::JoinGroup(first) = w.ask(5, Request::JoinGroup(req.clone())).await else {
        panic!("not JoinGroup")
    };
    assert_eq!(first.error_code, 79, "a v5 client is given an id first");
    req.member_id = first.member_id;
    let Response::JoinGroup(joined) = w.ask(5, Request::JoinGroup(req)).await else {
        panic!("not JoinGroup")
    };
    assert_eq!(joined.error_code, 0, "{joined:?}");
    let id = joined.member_id.to_string();
    sync(
        w,
        group,
        &id,
        joined.generation_id,
        &[(&id, assignment(&[("orders", &[0, 1, 2])]))],
    )
    .await;
    id
}

async fn leave_with_reason(w: &World, group: &str, member: &str, reason: &str) {
    let mut req = LeaveGroupRequest::default();
    req.group_id = GroupId(text(group));
    let mut who = MemberIdentity::default();
    who.member_id = text(member);
    who.reason = Some(text(reason));
    req.members = vec![who];
    let Response::LeaveGroup(r) = w.ask(5, Request::LeaveGroup(req)).await else {
        panic!("not LeaveGroup")
    };
    assert_eq!(r.members[0].error_code, 0);
}

#[tokio::test]
async fn a_classic_group_that_joins_and_leaves_tells_its_story_with_a_clipped_reason() {
    let w = instant("story-classic").await;
    let member = join_with_reason(&w, "fulfilment", "starting up").await;
    let huge = "r".repeat(900 * 1024);
    leave_with_reason(&w, "fulfilment", &member, &huge).await;
    let app = w.router();
    let story = ok(&app, "/groups/fulfilment/story", Some("group-story")).await;
    for want in [
        "Rebalances kept",
        "Member joined",
        "Generation 1",
        "joined an empty group and led it. Members 0 to 1.",
        "Member left",
        "Generation 2",
        "sent LeaveGroup. Members 1 to 0.",
        "Open this rebalance",
        "200 events per group",
    ] {
        assert!(story.contains(want), "{want:?} in {story:.6000}");
    }
    assert!(
        story.contains(&"r".repeat(128)),
        "the clipped reason is shown"
    );
    assert!(
        !story.contains(&"r".repeat(129)),
        "and only its first 128 bytes"
    );
    assert!(
        story.len() < 200_000,
        "a 900 KiB reason does not make a 900 KiB page: {}",
        story.len()
    );
    // The drawer of the rebalance the leave began: who, when, and what the client said.
    let drawer = ok(&app, "/groups/fulfilment/story/2", Some("story-rebalance")).await;
    assert!(
        drawer.contains("role=\"dialog\"") && drawer.contains("Generation 2"),
        "{drawer:.3000}"
    );
    assert!(drawer.contains(&"r".repeat(128)) && !drawer.contains(&"r".repeat(129)));
    assert!(
        drawer.contains(">Members<") && drawer.contains("1 before, 0 after"),
        "{drawer:.4000}"
    );
    // The tabs of the group's other pages carry the count.
    let members = ok(&app, "/groups/fulfilment/members", None).await;
    assert!(
        members.contains("Story<span class=\"n\">2</span>"),
        "{members:.3000}"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_script_gets_the_drawer_alone_and_a_browser_gets_the_story_with_it_open() {
    let w = instant("story-drawer").await;
    join_with_reason(&w, "g-drawer", "x").await;
    let app = w.router();
    let (status, alone) = page(&app, fragment_get("/groups/g-drawer/story/1")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        alone.starts_with("<div") && !alone.contains("<html"),
        "{alone:.300}"
    );
    let (status, whole) = page(&app, get("/groups/g-drawer/story/1")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        whole.contains("<html")
            && whole.contains("A quiet story is a good one")
            && whole.contains("data-overlay"),
        "{whole:.800}"
    );
    for gone in [
        "/groups/g-drawer/story/99",
        "/groups/g-drawer/story/abc",
        "/groups/g-drawer/story/-1",
        "/groups/g-drawer/story/%3Cscript%3E",
    ] {
        let (status, body) = page(&app, get(gone)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{gone}");
        assert!(!body.contains("<script>alert"), "{gone}");
    }
    w.stop().await;
}

fn create_partitions(topic: &str, count: i32) -> Request {
    kafka_console::backend::requests::topics_write::create_partitions(topic, count, false)
}

#[tokio::test]
async fn partitions_added_to_a_topic_a_consumer_group_reads_are_told_as_a_topic_that_changed() {
    let w = instant("story-848").await;
    join_848(&w, "payments-workers", "m-one", &["orders"]).await;
    let Response::CreatePartitions(r) = w.ask(3, create_partitions("orders", 5)).await else {
        panic!("not CreatePartitions")
    };
    assert_eq!(r.results[0].error_code, 0, "{r:?}");
    let app = w.router();
    let mut story = String::new();
    for _ in 0..100 {
        story = ok(&app, "/groups/payments-workers/story", Some("group-story")).await;
        if story.contains("Topic changed") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for want in [
        "Epoch 2",
        "Member joined",
        "Epoch 3",
        "Topic changed",
        "A topic this group reads changed",
    ] {
        assert!(story.contains(want), "{want:?} in {story:.7000}");
    }
    assert!(story.contains("m-one"), "the member who joined is named");
    // The group list says when the group last rebalanced.
    let list = ok(&app, "/groups", None).await;
    assert!(list.contains("last rebalance"), "{list:.3000}");
    w.stop().await;
}

#[tokio::test]
async fn a_member_that_expires_and_comes_back_until_the_rule_names_it_is_flapping() {
    let w = instant("story-flap").await;
    tokio::time::pause();
    let mut members = Vec::new();
    for _ in 0..3 {
        members.push(join_with_reason(&w, "flaky", "back").await);
        // Silent for longer than the session (45 s): the broker drops it, and the next one joins.
        tokio::time::advance(Duration::from_secs(50)).await;
        tokio::task::yield_now().await;
    }
    members.push(join_with_reason(&w, "flaky", "back").await);
    let app = w.router();
    let flap = ok(&app, "/groups/flaky/story/flapping", Some("story-flapping")).await;
    assert!(
        flap.contains("is <b>flapping</b>: 3 expired-then-joined cycles in the last 10 minutes."),
        "{flap:.5000}"
    );
    assert!(flap.contains("3 cycles"), "{flap:.5000}");
    let story = ok(&app, "/groups/flaky/story", None).await;
    assert!(
        story.contains("Session expired")
            && story.contains("Yes, by the rule: 3 expired-then-joined cycles"),
        "{story:.7000}"
    );
    // Clean leaves do not count, however often.
    for _ in 0..4 {
        let m = join_with_reason(&w, "calm", "x").await;
        leave_with_reason(&w, "calm", &m, "bye").await;
    }
    let calm = ok(&app, "/groups/calm/story/flapping", None).await;
    assert!(
        calm.contains("is <b>not</b> flapping") && calm.contains("0 cycles"),
        "{calm:.5000}"
    );
    assert!(calm.contains("A leave is not a timeout"));
    w.stop().await;
}

#[tokio::test]
async fn a_caller_who_may_not_describe_the_group_is_told_no_group_not_that_it_has_a_story() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    let w = with_orders("story-acl", |c| {
        c.acl_default_allow = false;
        c.group_initial_rebalance_delay = Duration::ZERO;
    })
    .await;
    join_with_reason(&w, "secret", "x").await;
    let someone = Caller::new(Principal::anonymous(), Some("127.0.0.1".parse().unwrap()));
    let app = router_of(&w, w.backend.acting_as(someone));
    for path in [
        "/groups/secret/story",
        "/groups/secret/story/1",
        "/groups/secret/story/flapping",
    ] {
        let (status, body) = page(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert!(
            !body.contains("Member joined"),
            "{path}: nothing of the story leaks"
        );
    }
    let (status, _) = page(&w.router(), get("/groups/secret/story")).await;
    assert_eq!(status, StatusCode::OK, "the console itself is a super user");
    w.stop().await;
}

#[tokio::test]
async fn a_hostile_group_id_is_escaped_and_encoded_on_the_story_pages() {
    let w = instant("story-hostile").await;
    let app = w.router();
    let long = "g".repeat(255);
    for id in [
        "a/b?c%d é 😀",
        "<script>alert(1)</script>",
        "..",
        "a\"b'c",
        &long,
    ] {
        let enc = support_write::enc(id);
        for tail in ["/story", "/story/1", "/story/flapping"] {
            let (status, body) = page(&app, get(&format!("/groups/{enc}{tail}"))).await;
            assert!(status.as_u16() < 500, "{id}{tail}: {status}");
            assert!(!body.contains("<script>alert"), "{id}{tail}");
        }
    }
    // A hostile id that IS a group (a member joined it) is shown as text on its story.
    let hostile = "<script>alert(1)</script>";
    let mut req = join_request(hostile, "", "range", &["orders"]);
    req.reason = Some(text("<b>x</b>"));
    let Response::JoinGroup(r) = w.ask(3, Request::JoinGroup(req)).await else {
        panic!("not JoinGroup")
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    let (status, body) = page(
        &app,
        get(&format!("/groups/{}/story", support_write::enc(hostile))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("<script>alert"), "{body:.1500}");
    assert!(body.contains("&lt;script&gt;"), "shown as text");
    w.stop().await;
}
