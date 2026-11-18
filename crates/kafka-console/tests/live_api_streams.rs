//! Streams groups (KIP-1071) on the Groups API, over a real broker in the same process with members that speak raw
//! StreamsGroupHeartbeat as a Kafka Streams word count does: the list names the type, the group's page has its topology
//! (subtopologies, internal topics with their partitions), its members' tasks by role and stage, and the lag of what it
//! committed on the topics it reads; a group whose source topic is missing says why it is not ready.
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
mod support_streams;
mod support_write;

use axum::Router;
use axum::http::StatusCode;
use serde_json::Value;
use support_api::{api, get};
use support_groups::with_orders;

async fn ok(app: &Router, path: &str) -> Value {
    let (s, v) = get(app, path).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

fn list_row<'a>(list: &'a Value, id: &str) -> &'a Value {
    list["view"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{id} is not listed: {list}"))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

/// CON-1550 to CON-1553: a streams group is listed with its type and described by StreamsGroupDescribe; its page has the
/// topology with the internal topics made for it, the member with its tasks by role, target and stage, and the lag of the
/// offsets it committed on the topics its tasks read.
#[tokio::test]
async fn a_streams_group_is_described_with_its_topology_tasks_and_lag() {
    let w = with_orders("ag-streams", |_| {}).await;
    let member = "streams-member-one";
    let epoch = support_streams::settle(&w, "counts", member, "orders").await;
    // Task 0_0 has read two of the four records of orders-0; 0_1 all three of orders-1.
    support_streams::commit(&w, ("counts", member, epoch), "orders", &[(0, 2), (1, 3)]).await;
    let app = api(&w);
    let list = ok(&app, "/api/v1/groups").await;
    let row = list_row(&list, "counts");
    assert_eq!(row["protocol"], "streams", "{row}");
    assert_eq!(row["phase"], "stable", "{row}");
    assert_eq!(row["members"], 1, "{row}");
    assert!(
        strings(&row["reads"]).contains(&"orders".to_owned()),
        "{row}"
    );

    let g = ok(&app, "/api/v1/groups/counts").await;
    let h = &g["head"];
    assert_eq!(h["protocol"], "streams", "{h}");
    assert_eq!(h["phase"], "stable", "{h}");
    assert_eq!(h["assignor"], "sticky", "{h}");
    assert_eq!(h["generation"].as_i64(), Some(i64::from(epoch)), "{h}");
    assert_eq!(h["members"][0]["id"], member, "{h}");
    // The member reads, at each partition, the topics of the subtopologies whose active tasks it runs.
    assert_eq!(h["members"][0]["holds"].as_array().unwrap().len(), 6, "{h}");

    let s = &g["streams"];
    assert_eq!(s["state"], "Stable", "{s}");
    assert_eq!(s["epoch"].as_i64(), Some(i64::from(epoch)), "{s}");
    assert_eq!(s["assignment_epoch"], s["epoch"], "{s}");
    assert_eq!(s["topology_epoch"], 0, "{s}");
    assert!(s["not_ready"].is_null(), "{s}");
    assert_eq!(s["live"], true, "{s}");
    let subs = s["subtopologies"].as_array().unwrap();
    assert_eq!(subs.len(), 2, "{s}");
    assert_eq!(strings(&subs[0]["sources"]), vec!["orders"], "{s}");
    assert_eq!(
        strings(&subs[0]["repartition_sinks"]),
        vec!["counts-words-repartition"],
        "{s}"
    );
    assert_eq!(
        strings(&subs[1]["repartition_sources"]),
        vec!["counts-words-repartition"],
        "{s}"
    );
    assert_eq!(
        strings(&subs[1]["changelogs"]),
        vec!["counts-counts-changelog"],
        "{s}"
    );
    assert_eq!(
        (subs[0]["tasks"].as_i64(), subs[1]["tasks"].as_i64()),
        (Some(3), Some(3)),
        "{s}"
    );
    let internal = s["internal"].as_array().unwrap();
    let kinds: Vec<(&str, &str, i64, i64)> = internal
        .iter()
        .map(|t| {
            let (name, kind) = (t["name"].as_str().unwrap(), t["kind"].as_str().unwrap());
            (
                name,
                kind,
                t["partitions"].as_i64().unwrap(),
                t["exists"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("counts-words-repartition", "repartition", 3, 3),
            ("counts-counts-changelog", "changelog", 3, 3)
        ],
        "made by the broker for the group: {s}"
    );
    let m = &s["members"][0];
    assert_eq!(m["id"], member, "{m}");
    assert_eq!(m["process"], format!("process-{member}"), "{m}");
    assert_eq!(m["endpoint"], "localhost:7070", "{m}");
    assert_eq!(m["stage"], "STABLE", "{m}");
    let all = vec!["0_0", "0_1", "0_2", "1_0", "1_1", "1_2"];
    assert_eq!(strings(&m["holds"]["active"]), all, "{m}");
    assert_eq!(strings(&m["target"]["active"]), all, "{m}");
    assert!(m["holds"]["standby"].as_array().unwrap().is_empty(), "{m}");
    assert!(
        m["revoking"]["active"].as_array().unwrap().is_empty(),
        "{m}"
    );

    // The lag table: what it committed on the topics its tasks read (orders and the repartition topic).
    let topics: Vec<&str> = g["topics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(topics, vec!["counts-words-repartition", "orders"], "{g}");
    let orders = g["topics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "orders")
        .unwrap();
    let lag: Vec<Value> = orders["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["lag"].clone())
        .collect();
    assert_eq!(
        lag[0],
        serde_json::json!({"kind": "behind", "n": 2}),
        "{orders}"
    );
    assert_eq!(lag[1], serde_json::json!({"kind": "caught_up"}), "{orders}");
    assert_eq!(lag[2], serde_json::json!({"kind": "no_commit"}), "{orders}");
    assert_eq!(h["lag_total"], 2, "{h}");
    w.stop().await;
}

/// CON-1551: a streams group whose source topic does not exist is NotReady, and the page says the status its members are
/// told, in Kafka's words.
#[tokio::test]
async fn a_streams_group_whose_source_topic_is_missing_says_why_it_is_not_ready() {
    let w = with_orders("ag-streams-missing", |_| {}).await;
    support_streams::quick(&w, "lonely").await;
    let r = support_streams::join(&w, "lonely", "lonely-member", "nowhere").await;
    assert!(
        r.status.iter().flatten().any(|s| s.status_code == 1),
        "{r:?}"
    );
    let app = api(&w);
    let g = ok(&app, "/api/v1/groups/lonely").await;
    assert_eq!(g["head"]["phase"], "not_ready", "{g}");
    let s = &g["streams"];
    assert_eq!(s["state"], "NotReady", "{s}");
    let why = &s["not_ready"];
    assert_eq!(why["code"], 1, "{why}");
    assert_eq!(why["name"], "MISSING_SOURCE_TOPICS", "{why}");
    assert!(why["detail"].as_str().unwrap().contains("nowhere"), "{why}");
    assert!(
        s["subtopologies"][0]["tasks"].is_null(),
        "no tasks before it is ready: {s}"
    );
    assert!(
        s["internal"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["exists"].is_null()),
        "{s}"
    );
    let list = ok(&app, "/api/v1/groups").await;
    assert_eq!(list_row(&list, "lonely")["phase"], "not_ready", "{list}");
    w.stop().await;
}
