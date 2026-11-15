//! The three Groups writes over a real broker in the same process, with real members (the rules CON-208 to CON-210 that drove
//! the server-rendered pages before R5): Delete group and Delete offsets done or refused in Kafka's name, number and words, and
//! a caller on an ACL-on broker refused as a client would be.
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

use axum::Router;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode, header};
use kafka_wire::api::{Request, Response};
use serde_json::Value;
use support::{HOST, send};
use support_api::{api, get};
use support_groups::*;

/// A write of the Groups API: JSON, with the console's proof.
async fn post(app: &Router, path: &str, body: &str) -> (StatusCode, Value) {
    let req = HttpRequest::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let (res, text) = send(app, req).await;
    (
        res.status(),
        serde_json::from_str(&text).unwrap_or(Value::Null),
    )
}

async fn ok(app: &Router, path: &str) -> Value {
    let (s, v) = get(app, path).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

/// CON-208: DeleteGroups on a group with a member is refused as NON_EMPTY_GROUP (68) in Kafka's words; an empty group is
/// deleted and is then no group.
#[tokio::test]
async fn deleting_a_group_is_refused_with_members_and_done_when_empty() {
    let w = with_orders("ag-delete", |_| {}).await;
    stable_group(&w, "search-indexer", "range", &[("orders", &[0, 1, 2])]).await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    let app = api(&w);
    let (s, r) = post(&app, "/api/v1/groups/search-indexer/delete", "{}").await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "refused", "{r}");
    assert_eq!(r["code"], "NON_EMPTY_GROUP");
    assert_eq!(r["number"], 68);
    assert_eq!(r["message"], "The group is not empty.");
    ok(&app, "/api/v1/groups/search-indexer").await;
    let (s, r) = post(&app, "/api/v1/groups/analytics/delete", "{}").await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "done", "{r}");
    let (gone, e) = get(&app, "/api/v1/groups/analytics").await;
    assert_eq!(gone, StatusCode::NOT_FOUND, "{e}");
    assert!(committed(&w, "analytics").await.is_empty());
    w.stop().await;
}

/// CON-209: OffsetDelete on an empty group forgets the commits; on a topic a member is subscribed to Kafka answers
/// GROUP_SUBSCRIBED_TO_TOPIC (86) and the commit stays.
#[tokio::test]
async fn deleting_offsets_forgets_them_and_a_subscribed_topic_is_refused() {
    let w = with_orders("ag-offdel", |_| {}).await;
    commit(&w, "analytics", "orders", &[(0, 1), (1, 1)]).await;
    let member = stable_group(&w, "search-indexer", "range", &[("orders", &[0, 1, 2])]).await;
    commit_as_member(&w, "search-indexer", &member, 1, &[(0, 1)]).await;
    let app = api(&w);
    let (s, r) = post(
        &app,
        "/api/v1/groups/analytics/offsets/delete",
        r#"{"topics":["orders"]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "done", "{r}");
    assert!(committed(&w, "analytics").await.is_empty());
    let (s, r) = post(
        &app,
        "/api/v1/groups/search-indexer/offsets/delete",
        r#"{"topics":["orders"]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "refused", "{r}");
    assert_eq!(r["code"], "GROUP_SUBSCRIBED_TO_TOPIC");
    assert_eq!(r["number"], 86);
    assert!(r["offsets"].is_object(), "the dialog's data again: {r}");
    assert_eq!(
        committed(&w, "search-indexer").await.len(),
        1,
        "nothing was forgotten"
    );
    w.stop().await;
}

/// CON-210: with ACLs on, a caller that may only DESCRIBE a group is refused a reset and a delete with
/// GROUP_AUTHORIZATION_FAILED (30), and a group it may not describe is the 404 of no group.
#[tokio::test]
async fn on_a_broker_with_acls_a_caller_without_the_right_is_refused_as_a_client_would_be() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    let w = with_orders("ag-acl", |b| b.acl_default_allow = false).await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    commit(&w, "search-indexer", "orders", &[(0, 1)]).await;
    let acl = |res: (i8, &str), op: i8| {
        let mut a = kafka_wire::messages::create_acls_request::AclCreation::default();
        a.resource_type = res.0;
        a.resource_name = kafka_wire::StrBytes::from_string(res.1.to_owned());
        a.resource_pattern_type = 3;
        a.principal = kafka_wire::StrBytes::from_static_str("User:ANONYMOUS");
        a.host = kafka_wire::StrBytes::from_static_str("*");
        a.operation = op;
        a.permission_type = 3;
        a
    };
    // DESCRIBE (8) on the group analytics and the topic orders, nothing else.
    let mut grant = kafka_wire::messages::CreateAclsRequest::default();
    grant.creations = vec![acl((3, "analytics"), 8), acl((2, "orders"), 8)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(grant)).await else {
        panic!()
    };
    assert!(r.results.iter().all(|x| x.error_code == 0), "{r:?}");
    let someone = Caller::new(Principal::anonymous(), Some("127.0.0.1".parse().unwrap()));
    let app = support_write::router_of(&w, w.backend.acting_as(someone));
    let g = ok(&app, "/api/v1/groups/analytics").await;
    assert_eq!(g["head"]["id"], "analytics");
    let (s, reset) = post(
        &app,
        "/api/v1/groups/analytics/reset",
        r#"{"topic":"orders","to":"earliest","value":""}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{reset}");
    assert_eq!(reset["outcome"], "refused", "{reset}");
    assert_eq!(reset["code"], "GROUP_AUTHORIZATION_FAILED");
    assert_eq!(reset["number"], 30);
    assert_eq!(reset["message"], "Group authorization failed.");
    let (_, del) = post(&app, "/api/v1/groups/analytics/delete", "{}").await;
    assert_eq!(del["outcome"], "refused", "{del}");
    assert_eq!(del["code"], "GROUP_AUTHORIZATION_FAILED");
    assert_eq!(committed(&w, "analytics").await.len(), 1, "nothing changed");
    // A group it may not describe is not told apart from no group.
    let (hidden, a) = get(&app, "/api/v1/groups/search-indexer").await;
    let (none, b) = get(&app, "/api/v1/groups/no-such-group").await;
    assert_eq!(hidden, StatusCode::NOT_FOUND, "{a}");
    assert_eq!(none, StatusCode::NOT_FOUND);
    assert_eq!(a["code"], b["code"]);
    let list = ok(&app, "/api/v1/groups").await;
    assert!(!list.to_string().contains("search-indexer"), "{list}");
    // The console itself is a super user and sees it.
    ok(&api(&w), "/api/v1/groups/search-indexer").await;
    w.stop().await;
}
