//! The Story API of a group over a real broker in the same process, with real members (the rules CON-606, CON-612, CON-615,
//! CON-616 and CON-617 that drove the server-rendered Story before R5): the generations newest first from the ring, what the
//! ring keeps and when it is full, a quiet story, a caller that may not describe the group, and group ids that are hostile.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_groups;
mod support_write;

use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::leave_group_request::MemberIdentity;
use kafka_wire::messages::{GroupId, LeaveGroupRequest};
use serde_json::Value;
use support_api::{api, get};
use support_groups::*;
use support_write::{World, enc};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A broker whose groups form at once (no initial wait) with `orders` made.
async fn instant(name: &str, acl: bool) -> World {
    with_orders(name, |c| {
        c.group_initial_rebalance_delay = Duration::ZERO;
        if acl {
            c.acl_default_allow = false;
        }
    })
    .await
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

async fn story(app: &Router, group: &str) -> Value {
    let path = format!("/api/v1/groups/{}/story", enc(group));
    let (s, v) = get(app, &path).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

fn generations(v: &Value) -> &Vec<Value> {
    v["generations"].as_array().unwrap()
}

/// CON-606: the Story is the group's generations, newest first, each with its cause, who (and the client's clipped reason),
/// the members before and after, how long it took to settle, what each member held and how long it lasted; the causes with
/// how often each happened; the flapping verdict; another group that rebalances rarely, for contrast.
#[tokio::test]
async fn the_story_is_the_generations_newest_first_from_the_ring() {
    let w = instant("as-story", false).await;
    let calm = join_with_reason(&w, "search-indexer", "steady").await;
    let member = join_with_reason(&w, "fulfilment", "starting up").await;
    let huge = "r".repeat(900 * 1024);
    leave_with_reason(&w, "fulfilment", &member, &huge).await;
    let app = api(&w);
    let v = story(&app, "fulfilment").await;
    assert_eq!(v["in_memory"], true);
    let g = generations(&v);
    let seen: Vec<_> = g
        .iter()
        .map(|g| {
            (
                g["generation"].as_i64().unwrap(),
                g["cause"].as_str().unwrap().to_owned(),
                g["before"].as_i64().unwrap(),
                g["after"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (2, "member_left".to_owned(), 1, 0),
            (1, "member_joined".to_owned(), 0, 1),
        ],
        "newest first: {v}"
    );
    let (left, joined) = (&g[0], &g[1]);
    assert_eq!(left["who"]["member"], member.as_str());
    assert_eq!(left["who"]["client"], "test");
    let reason = left["who"]["reason"].as_str().unwrap();
    assert_eq!(
        reason,
        "r".repeat(128),
        "the client's reason, clipped to 128 bytes"
    );
    assert_eq!(joined["who"]["reason"], "starting up");
    assert_eq!(joined["leader"], member.as_str());
    assert!(joined["settled_ms"].as_i64().is_some(), "{joined}");
    assert!(joined["lasted_ms"].as_i64().is_some(), "{joined}");
    assert_eq!(joined["members"], serde_json::json!([member]));
    assert_eq!(
        joined["holds"][0]["topics"],
        serde_json::json!([["orders", [0, 1, 2]]]),
        "what the member held: {joined}"
    );
    let mut causes: Vec<_> = v["causes"].as_array().unwrap().clone();
    causes.sort_by_key(ToString::to_string);
    assert_eq!(
        causes,
        vec![
            serde_json::json!(["member_joined", 1]),
            serde_json::json!(["member_left", 1])
        ]
    );
    assert_eq!(v["flap"]["flapping"], false);
    assert_eq!(v["flap"]["needs"], 3);
    // Another group, quiet, for contrast: one rebalance, its first member.
    assert_eq!(v["calm"]["group"], "search-indexer", "{v}");
    assert_eq!(v["calm"]["count"], 1);
    assert_eq!(v["calm"]["generation"]["who"]["member"], calm.as_str());
    assert!(
        v.to_string().len() < 200_000,
        "a 900 KiB reason does not make a 900 KiB answer"
    );
    // The head's count is the ring's (what a tab shows).
    assert_eq!(v["head"]["story"]["count"], 2);
    w.stop().await;
}

/// CON-612: the Story says what the ring is: 200 events per group, emptied at a restart; when it is full the oldest
/// generations are gone and the answer says so (`full`, and the oldest generation it holds is not the first).
#[tokio::test]
async fn the_story_says_what_the_ring_keeps_and_when_it_is_full() {
    let w = instant("as-ring", false).await;
    let app = api(&w);
    let m = join_with_reason(&w, "churn", "first").await;
    let v = story(&app, "churn").await;
    assert_eq!(v["events_per_group"], 200);
    assert_eq!(v["full"], false, "one join is not a full ring");
    leave_with_reason(&w, "churn", &m, "x").await;
    let mut v = Value::Null;
    for _ in 0..400 {
        let m = join_with_reason(&w, "churn", "again").await;
        leave_with_reason(&w, "churn", &m, "x").await;
        v = story(&app, "churn").await;
        if v["full"] == true {
            break;
        }
    }
    assert_eq!(v["full"], true, "the ring filled: {:.400}", v.to_string());
    // Full, it still holds the first; the next rebalances push the oldest out.
    assert_eq!(generations(&v).last().unwrap()["generation"], 1);
    for _ in 0..3 {
        let m = join_with_reason(&w, "churn", "more").await;
        leave_with_reason(&w, "churn", &m, "x").await;
    }
    let v = story(&app, "churn").await;
    assert_eq!(v["full"], true);
    let gens = generations(&v);
    let oldest = gens.last().unwrap()["generation"].as_i64().unwrap();
    let newest = gens[0]["generation"].as_i64().unwrap();
    assert!(
        oldest > 1,
        "generation 1 is older than the ring keeps: {oldest}"
    );
    assert!(newest > oldest);
    assert!(
        gens.windows(2)
            .all(|p| p[0]["generation"].as_i64() > p[1]["generation"].as_i64()),
        "newest first"
    );
    assert_eq!(v["events_per_group"], 200);
    w.stop().await;
}

/// CON-615: a group id is hostile input: it is one path segment, percent-encoded, and comes back as the same JSON text; an
/// id that is no group is the 404 of no group, and nothing is echoed as anything but text.
#[tokio::test]
async fn a_hostile_group_id_is_text_on_the_story() {
    let w = instant("as-hostile", false).await;
    let app = api(&w);
    let long = "g".repeat(255);
    for id in [
        "a/b?c%d é 😀",
        "<script>alert(1)</script>",
        "..",
        "a\"b'c",
        &long,
    ] {
        let (s, v) = get(&app, &format!("/api/v1/groups/{}/story", enc(id))).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{id}: {v}");
        assert_eq!(v["code"], "GROUP_NOT_FOUND");
    }
    let hostile = "<script>alert(1)</script>";
    let mut req = join_request(hostile, "", "range", &["orders"]);
    req.reason = Some(text("<b>x</b>"));
    let Response::JoinGroup(r) = w.ask(3, Request::JoinGroup(req)).await else {
        panic!("not JoinGroup")
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    let v = story(&app, hostile).await;
    assert_eq!(v["head"]["id"], hostile);
    assert_eq!(generations(&v)[0]["generation"], 1);
    w.stop().await;
}

/// CON-616: the Story of a group the caller may not DESCRIBE is the 404 of a group that is not there; nothing of it leaks.
#[tokio::test]
async fn a_caller_who_may_not_describe_the_group_is_told_no_group_not_that_it_has_a_story() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    let w = instant("as-acl", true).await;
    let member = join_with_reason(&w, "secret", "x").await;
    let someone = Caller::new(Principal::anonymous(), Some("127.0.0.1".parse().unwrap()));
    let app = support_write::router_of(&w, w.backend.acting_as(someone));
    let (s, hidden) = get(&app, "/api/v1/groups/secret/story").await;
    let (_, none) = get(&app, "/api/v1/groups/never-was/story").await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{hidden}");
    assert_eq!(hidden["code"], none["code"]);
    assert!(!hidden.to_string().contains(&member), "nothing leaks");
    assert!(!hidden.to_string().contains("member_joined"));
    // The console itself is a super user and has it.
    let v = story(&api(&w), "secret").await;
    assert_eq!(generations(&v).len(), 1);
    w.stop().await;
}

/// CON-617: a group with one rebalance and nothing since is a quiet story: stable, one generation, none forming, in memory
/// (what the page's `quiet` reads), its only cause the join.
#[tokio::test]
async fn a_group_with_one_rebalance_and_nothing_since_has_a_quiet_story() {
    let w = instant("as-quiet", false).await;
    let member = join_with_reason(&w, "search-indexer", "x").await;
    let app = api(&w);
    let v = story(&app, "search-indexer").await;
    assert_eq!(v["in_memory"], true);
    assert_eq!(v["head"]["phase"], "stable");
    let g = generations(&v);
    assert_eq!(g.len(), 1, "{v}");
    assert_eq!(g[0]["forming"], false);
    assert_eq!(g[0]["cause"], "member_joined");
    assert_eq!(g[0]["who"]["member"], member.as_str());
    assert_eq!(v["causes"], serde_json::json!([["member_joined", 1]]));
    assert!(v["now"]["waiting_for"].as_array().unwrap().is_empty());
    // A second member makes it a story with more than one rebalance.
    let j2 = tokio::spawn({
        let broker = std::sync::Arc::clone(&w.broker);
        async move { join_in_background(&broker, "search-indexer", "range", &["orders"]).await }
    });
    let mut v = Value::Null;
    for _ in 0..100 {
        v = story(&app, "search-indexer").await;
        if generations(&v).len() > 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(generations(&v).len(), 2, "{v}");
    assert_eq!(generations(&v)[0]["forming"], true, "{v}");
    j2.abort();
    w.stop().await;
}
