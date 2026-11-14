//! The Groups API over a real broker in the same process, with real members (slice R2's answers of the rules CON-201 to
//! CON-211 that drove the server-rendered pages before R5): classic and KIP-848 groups joined through the group machines,
//! commits and none, a topic recreated under a commit, a rebalance in progress, and group ids that are hostile input. The
//! writes' refusals and the ACL-on broker are in `live_api_group_writes.rs`.
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
use kafka_wire::messages::{DeleteTopicsRequest, TopicName};
use serde_json::Value;
use support::{HOST, send};
use support_api::{api, get};
use support_groups::*;
use support_write::{World, enc};

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

fn rows(v: &Value) -> Vec<&Value> {
    v["topics"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["rows"].as_array().unwrap())
        .collect()
}

fn list_row<'a>(list: &'a Value, id: &str) -> &'a Value {
    list["view"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{id} is not listed: {list}"))
}

/// No figure of lag the API sends is negative, anywhere in the answer.
fn no_negative_lag(v: &Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if matches!(k.as_str(), "n" | "total" | "lag_total" | "behind") {
                    assert!(x.as_i64().is_none_or(|n| n >= 0), "{k} = {x}");
                }
                no_negative_lag(x);
            }
        }
        Value::Array(a) => a.iter().for_each(no_negative_lag),
        _ => {}
    }
}

async fn recreate_orders(w: &World) {
    let mut d = DeleteTopicsRequest::default();
    d.topic_names = vec![TopicName(kafka_wire::StrBytes::from_static_str("orders"))];
    d.timeout_ms = 5000;
    let Response::DeleteTopics(r) = w.ask(6, Request::DeleteTopics(d)).await else {
        panic!()
    };
    assert_eq!(r.responses[0].error_code, 0, "{r:?}");
    w.create_topic("orders", 3, &[]).await;
}

/// CON-201: commits made after a topic was deleted and made again, shorter, are `past_end` by how far, on the group and on the
/// list (`ahead`), and never a negative lag.
#[tokio::test]
async fn a_commit_beyond_the_end_of_a_recreated_topic_is_past_the_end_never_negative_lag() {
    let w = with_orders("ag-recreate", |_| {}).await;
    recreate_orders(&w).await;
    produce(&w, "orders", 0, 1).await;
    commit(&w, "analytics", "orders", &[(0, 4), (1, 3), (2, 2)]).await;
    let app = api(&w);
    let g = ok(&app, "/api/v1/groups/analytics").await;
    assert_eq!(g["state"], "ahead", "{g}");
    let r = rows(&g);
    let seen: Vec<_> = r
        .iter()
        .map(|r| {
            (
                r["partition"].as_i64().unwrap(),
                r["end"].as_i64().unwrap(),
                r["lag"].clone(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (0, 1, serde_json::json!({"kind": "past_end", "n": 3})),
            (1, 0, serde_json::json!({"kind": "past_end", "n": 3})),
            (2, 0, serde_json::json!({"kind": "past_end", "n": 2})),
        ]
    );
    assert_eq!(g["head"]["lag_total"], 0, "past the end is not lag");
    no_negative_lag(&g);
    let list = ok(&app, "/api/v1/groups").await;
    let row = list_row(&list, "analytics");
    assert_eq!(
        row["lag"]["ahead"],
        serde_json::json!(["orders-0", 3]),
        "{row}"
    );
    no_negative_lag(&list);
    w.stop().await;
}

/// CON-202: a group whose member reads a topic and has committed nothing has `no_commit` partitions and the state
/// `no_commits`, never the log end as lag; the list says it has no commit and which topics are uncommitted.
#[tokio::test]
async fn a_group_with_no_commit_shows_no_commits_and_not_the_log_end_as_lag() {
    let w = with_orders("ag-nocommit", |_| {}).await;
    stable_group(&w, "fulfilment", "range", &[("orders", &[0, 1, 2])]).await;
    let app = api(&w);
    let g = ok(&app, "/api/v1/groups/fulfilment").await;
    assert_eq!(g["state"], "no_commits", "{g}");
    let r = rows(&g);
    assert_eq!(r.len(), 3, "{g}");
    for row in r {
        assert_eq!(
            row["lag"],
            serde_json::json!({"kind": "no_commit"}),
            "{row}"
        );
        assert!(row["committed"].is_null(), "{row}");
    }
    assert_eq!(g["head"]["lag_total"], 0, "the log end (9) is not lag");
    assert_eq!(g["head"]["commits"], 0);
    let list = ok(&app, "/api/v1/groups").await;
    let row = list_row(&list, "fulfilment");
    assert_eq!(row["lag"]["any_commit"], false, "{row}");
    assert_eq!(row["lag"]["total"], 0, "{row}");
    assert_eq!(
        row["lag"]["uncommitted"],
        serde_json::json!(["orders"]),
        "{row}"
    );
    w.stop().await;
}

/// CON-204: what the protocol hides (the leader, the generation, each member's session and rebalance timeouts and when it
/// was heard, and for a rebalance who it waits for) comes from the group's own task.
#[tokio::test]
async fn a_classic_group_with_a_member_and_commits_shows_what_the_protocol_hides() {
    let w = with_orders("ag-classic", |_| {}).await;
    let member = stable_group(
        &w,
        "search-indexer",
        "roundrobin",
        &[("orders", &[0, 1, 2])],
    )
    .await;
    commit_as_member(&w, "search-indexer", &member, 1, &[(0, 1), (1, 3), (2, 2)]).await;
    let app = api(&w);
    let g = ok(&app, "/api/v1/groups/search-indexer").await;
    let h = &g["head"];
    assert_eq!(h["protocol"], "classic");
    assert_eq!(h["assignor"], "roundrobin");
    assert_eq!(h["generation"], 1);
    assert_eq!(h["leader"], member.as_str());
    assert_eq!(h["phase"], "stable");
    let m = &h["members"][0];
    assert_eq!(m["id"], member.as_str());
    assert_eq!(m["leader"], true);
    assert_eq!(m["session_ms"], 45_000, "the JoinGroup's session timeout");
    assert_eq!(m["rebalance_ms"], 60_000, "and its rebalance timeout");
    let heard = m["heard_ago_ms"].as_i64().unwrap();
    assert!((0..30_000).contains(&heard), "{m}");
    assert_eq!(m["holds"].as_array().unwrap().len(), 3);
    // orders ends at 4, 3, 2; the group committed 1, 3, 2: 3 behind in orders-0, caught up elsewhere.
    let lags: Vec<_> = rows(&g).iter().map(|r| r["lag"].clone()).collect();
    assert_eq!(
        lags,
        vec![
            serde_json::json!({"kind": "behind", "n": 3}),
            serde_json::json!({"kind": "caught_up"}),
            serde_json::json!({"kind": "caught_up"}),
        ]
    );
    assert_eq!(h["lag_total"], 3);
    assert_eq!(h["commits"], 3);
    assert!(h["rebalance"].is_null(), "{h}");
    w.stop().await;
}

/// CON-204: a rebalance in progress says who it waits for and when it began.
#[tokio::test]
async fn a_rebalance_in_progress_says_who_it_waits_for() {
    let w = with_orders("ag-rebalance", |_| {}).await;
    let first = stable_group(&w, "fulfilment", "range", &[("orders", &[0, 1, 2])]).await;
    // A second member joins: the group must rebalance, and the first has to join again. Its join waits for the group.
    let j2 = tokio::spawn({
        let broker = std::sync::Arc::clone(&w.broker);
        async move { join_in_background(&broker, "fulfilment", "range", &["orders"]).await }
    });
    let app = api(&w);
    let mut g = Value::Null;
    for _ in 0..100 {
        g = ok(&app, "/api/v1/groups/fulfilment").await;
        if g["head"]["rebalance"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let r = &g["head"]["rebalance"];
    assert!(r.is_object(), "{g}");
    assert_eq!(g["head"]["phase"], "rebalancing", "{g}");
    assert!(
        r.to_string().contains(&first),
        "the member it waits for is named: {r}"
    );
    let list = ok(&app, "/api/v1/groups").await;
    assert!(
        list["attention"].to_string().contains("fulfilment"),
        "the list's line names it: {list}"
    );
    j2.abort();
    w.stop().await;
}

/// CON-205: a consumer group of the new protocol is Dead to DescribeGroups; the API asks ConsumerGroupDescribe and has its
/// epoch, target epoch, assignor, and each member's target and assigned partitions.
#[tokio::test]
async fn a_consumer_group_of_the_new_protocol_is_described_by_its_own_api() {
    let w = with_orders("ag-848", |_| {}).await;
    join_848(&w, "payments-workers", "m-one", &["orders"]).await;
    let app = api(&w);
    let g = ok(&app, "/api/v1/groups/payments-workers").await;
    let h = &g["head"];
    assert_eq!(h["protocol"], "consumer", "{h}");
    assert_eq!(h["phase"], "stable", "not Dead: {h}");
    assert_eq!(h["assignor"], "uniform");
    let epoch = h["generation"].as_i64().unwrap();
    assert!(epoch >= 1, "{h}");
    assert_eq!(h["target_epoch"].as_i64(), Some(epoch), "{h}");
    let m = &h["members"][0];
    assert_eq!(m["id"], "m-one");
    let parts = |k: &str| -> Vec<(String, i64)> {
        let mut v: Vec<_> = m[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["topic"].as_str().unwrap().to_owned(),
                    p["partition"].as_i64().unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    };
    let all: Vec<_> = (0..3).map(|p| ("orders".to_owned(), p)).collect();
    assert_eq!(parts("target"), all, "{m}");
    assert_eq!(parts("holds"), all, "{m}");
    let list = ok(&app, "/api/v1/groups").await;
    assert_eq!(list_row(&list, "payments-workers")["protocol"], "consumer");
    w.stop().await;
}

/// CON-211: a group id with `/`, `?`, `%`, spaces, unicode, a `<script>`, an attribute break or 255 bytes is one path segment
/// percent-encoded, reaches the broker as it is, and comes back as the same JSON text on every address of the group.
#[tokio::test]
async fn a_hostile_group_id_is_one_segment_and_comes_back_as_the_same_text() {
    let w = with_orders("ag-hostile", |_| {}).await;
    let long = "g".repeat(255);
    let app = api(&w);
    for id in [
        "a/b?c%d é 😀",
        "<script>alert(1)</script>",
        "x\"onmouseover=\"y",
        "..",
        &long,
    ] {
        commit(&w, id, "orders", &[(0, 1)]).await;
        let e = enc(id);
        for tail in [
            "",
            "?expiring=1",
            "/story",
            "/reset?topic=orders&to=earliest",
            "/delete",
            "/offsets/delete",
        ] {
            let path = format!("/api/v1/groups/{e}{tail}");
            let v = ok(&app, &path).await;
            if matches!(tail, "" | "?expiring=1" | "/story") {
                assert_eq!(
                    v["head"]["id"], id,
                    "{path}: the id comes back as the same text"
                );
            }
            assert!(v.is_object(), "{path}");
        }
        let g = ok(&app, &format!("/api/v1/groups/{e}")).await;
        assert_eq!(g["head"]["id"], id);
        let list = ok(&app, "/api/v1/groups").await;
        list_row(&list, id);
        // Its offsets go as a write of that group, not of another one.
        let (_, r) = post(
            &app,
            &format!("/api/v1/groups/{e}/offsets/delete"),
            r#"{"topics":["orders"]}"#,
        )
        .await;
        assert_eq!(r["outcome"], "done", "{id}: {r}");
        assert!(committed(&w, id).await.is_empty(), "{id}");
    }
    w.stop().await;
}
