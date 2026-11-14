//! The Lab's teaching over a real broker: the guide's recipes name the broker's own topic and app, Stop all ends every rule in
//! force, and Live's `lab` filter keeps only the requests a rule touched (CON-1370 to CON-1375).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    unused_imports
)]

mod support;
mod support_lab;
mod support_log;
mod support_write;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_console::app::console;
use kafka_console::base::Base;
use kafka_console::limits::Limits;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_wire::messages::{ApiKey, MetadataRequest};
use serde_json::Value;
use support::{HOST, send};
use support_lab::{Raw, fetch_req, lab_world, produce_req};
use tokio_util::sync::CancellationToken;

fn app_of(source: Source) -> Router {
    console(State::console(
        source,
        &Base::root(),
        vec![],
        CancellationToken::new(),
        &Limits::default(),
    ))
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap()
}

fn post(path: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn json(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let (res, text) = send(app, req).await;
    let status = res.status();
    (
        status,
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("{text}")),
    )
}

fn recipe<'a>(guide: &'a Value, id: &str) -> &'a Value {
    guide["recipes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{id}: {guide}"))
}

/// CON-1370 to CON-1373: the guide teaches every rehearsal, and its recipes name what the broker has: the topic written to and
/// the app writing it; a recipe the broker cannot serve says what it needs. Without the Lab: the words, and no recipe.
#[tokio::test]
async fn the_guide_teaches_and_its_recipes_name_the_brokers_own_topic_and_app() {
    let l = lab_world("guide-recipes").await;
    let live = Live::new(l.w.backend.clone(), l.w.facts.clone())
        .with_recorder(Arc::clone(&l.rec))
        .with_lab(Arc::clone(l.lab.table()));
    let app = app_of(Source::Live(Box::new(live)));
    let mut c = Raw::connect(&l.w, "payments").await;
    for _ in 0..3 {
        c.try_call(ApiKey::Produce, 9, &produce_req("orders"))
            .await
            .unwrap();
    }
    let (s, g) = json(&app, get("/api/v1/lab/guide")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(g["intro"]["steps"].as_array().unwrap().len(), 3, "{g}");
    let tabs: Vec<&str> = g["rehearsals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["tab"].as_str().unwrap())
        .collect();
    assert_eq!(
        tabs,
        ["slow", "fail", "drop", "expire", "rebalance", "compact"]
    );
    let p = recipe(&g, "slow-produce");
    assert_eq!(p["title"], "Slow every Produce to orders by 800 ms");
    assert_eq!(p["fields"]["topic"], "orders");
    assert!(p["needs"].is_null());
    assert_eq!(recipe(&g, "fail-timeout")["fields"]["client"], "payments");
    assert_eq!(recipe(&g, "drop-once")["target"], "payments");
    assert_eq!(
        recipe(&g, "rebalance-group")["needs"],
        "Needs a group with members."
    );
    // The recipe's fields make the rule the preview says, in plain words.
    let (_, v) = json(
        &app,
        post(
            "/api/v1/lab/rules",
            &serde_json::json!({"kind": "slow", "fields": p["fields"], "check": true}),
        ),
    )
    .await;
    assert_eq!(
        v["form"]["preview"]["plain"],
        "Every Produce request to orders will wait 800 ms longer before the broker answers. It ends after 10 min, or when you remove it.",
        "{v}"
    );
    // The fixtures have no Lab: the words, no recipe.
    let (s, g) = json(&app_of(Source::Fixture), get("/api/v1/lab/guide")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(g["rehearsals"].as_array().unwrap().len(), 6);
    assert!(g["recipes"].as_array().unwrap().is_empty());
    l.w.stop().await;
}

/// CON-1374, CON-1375: Stop all ends every rule in force (they go to Ended); Live's `lab` filter keeps only the requests a rule
/// touched, that rule's (`lab=<id>`) or any rule's (`lab=all`), and says so in its phrases.
#[tokio::test]
async fn stop_all_ends_every_rule_and_live_filters_by_the_lab_tag() {
    let l = lab_world("guide-stop").await;
    let live = Live::new(l.w.backend.clone(), l.w.facts.clone())
        .with_recorder(Arc::clone(&l.rec))
        .with_lab(Arc::clone(l.lab.table()));
    let app = app_of(Source::Live(Box::new(live)));
    let mut ids = Vec::new();
    for (kind, fields) in [
        (
            "fail",
            serde_json::json!({"api": "Produce", "topic": "orders", "share": "every", "ends": "5", "code": "7"}),
        ),
        (
            "slow",
            serde_json::json!({"api": "Metadata", "delay": "5", "share": "every", "ends": "5"}),
        ),
    ] {
        let (_, v) = json(
            &app,
            post(
                "/api/v1/lab/rules",
                &serde_json::json!({"kind": kind, "fields": fields}),
            ),
        )
        .await;
        assert_eq!(v["outcome"], "made", "{v}");
        ids.push(v["id"].as_u64().unwrap());
    }
    let mut c = Raw::connect(&l.w, "lab-filter").await;
    c.try_call(ApiKey::Produce, 9, &produce_req("orders"))
        .await
        .unwrap();
    c.try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
        .await
        .unwrap();
    // A request no rule matches.
    c.try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    let rows = |v: &Value| v["live"]["rows"].as_array().unwrap().clone();
    let (_, all) = json(&app, get("/api/v1/live?idle=0")).await;
    let (_, any) = json(&app, get("/api/v1/live?lab=all&idle=0")).await;
    let (_, one) = json(&app, get(&format!("/api/v1/live?lab={}&idle=0", ids[0]))).await;
    assert!(
        rows(&all).len() > rows(&any).len(),
        "the Fetch is not a lab row: {all}"
    );
    assert!(rows(&any).iter().all(|r| !r["lab"].is_null()));
    assert_eq!(rows(&any).len(), 2, "{any}");
    assert_eq!(rows(&one).len(), 1);
    assert_eq!(rows(&one)[0]["lab"]["rule"].as_u64(), Some(ids[0]));
    assert_eq!(any["words"]["phrases"][0], "a Lab rule touched it");
    assert_eq!(
        one["words"]["phrases"][0],
        format!("Lab rule {} touched it", ids[0])
    );
    let (s, v) = json(&app, post("/api/v1/lab/stop-all", &serde_json::json!({}))).await;
    assert_eq!((s, v["stopped"].as_u64()), (StatusCode::OK, Some(2)), "{v}");
    let (_, v) = json(&app, get("/api/v1/lab")).await;
    assert!(v["home"]["active"].as_array().unwrap().is_empty(), "{v}");
    assert_eq!(v["home"]["ended"].as_array().unwrap().len(), 2);
    let (_, v) = json(&app, post("/api/v1/lab/stop-all", &serde_json::json!({}))).await;
    assert_eq!(v["stopped"], 0, "nothing left to stop");
    // Without the Lab, Stop all says so.
    let (s, v) = json(
        &app_of(Source::Fixture),
        post("/api/v1/lab/stop-all", &serde_json::json!({})),
    )
    .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("LAB_OFF"))
    );
    l.w.stop().await;
}
