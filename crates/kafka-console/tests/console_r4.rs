//! The JSON API of Settings, Access and the Lab (slice R4): what each GET answers, what each write comes to (in Kafka's or the
//! engine's words), the proof a write needs, and the Lab's rule form taking several APIs, topics, partitions and a group.
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
use kafka_console::lab::form::spec_of;
use kafka_console::limits::Limits;
use kafka_console::model::lab::{LabFields, LabKind};
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_server::fault::{Effect, Share};
use kafka_wire::messages::{ApiKey, ProduceResponse};
use serde_json::Value;
use support::{HOST, send};
use support_lab::{Raw, body, lab_world, produce_req};
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

/// CON-1300, CON-1301: Settings is the page's view-model, without a memory figure, and its dialog is the five configs'.
#[tokio::test]
async fn the_settings_api_answers_the_page_and_the_edit_dialog() {
    let app = app_of(Source::Fixture);
    let (s, v) = json(&app, get("/api/v1/settings")).await;
    assert_eq!(s, StatusCode::OK);
    let rows = v["settings"]["configs"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5, "{v}");
    assert!(rows.iter().all(|r| r["source"].is_string()), "{v}");
    let text = v.to_string();
    // No memory figure: the weight has no `resident` (nor its neighbour `threads`) key.
    let weight = v["settings"]["weight"].as_object().unwrap();
    assert!(!text.contains("resident"), "{text}");
    assert!(!weight.contains_key("threads"), "{weight:?}");
    let (s, v) = json(&app, get("/api/v1/settings/edit?key=log.retention.ms")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["key"], "log.retention.ms");
    assert_eq!(v["keys"].as_array().unwrap().len(), 5);
    assert!(
        v["call"].to_string().contains("IncrementalAlterConfigs"),
        "{v}"
    );
    let (s, v) = json(&app, get("/api/v1/settings/edit?key=num.partitions")).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("NOT_FOUND"))
    );
    // An empty value is the dialog again, with its sentence.
    let (s, v) = json(
        &app,
        post(
            "/api/v1/settings/edit",
            &serde_json::json!({"key": "log.retention.ms", "scope": "all", "value": " ", "check": true}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["outcome"], "invalid", "{v}");
    assert_eq!(v["form"]["error"], "Type a value.");
    // Not JSON: refused in JSON.
    let bad = Request::builder()
        .method("POST")
        .uri("/api/v1/settings/edit")
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::from("key=x"))
        .unwrap();
    let (s, v) = json(&app, bad).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("BAD_REQUEST"))
    );
}

/// CON-1310, CON-1311: Access is the page's view-model with the form's lists; a user is a name and counts, never a secret.
#[tokio::test]
async fn the_access_api_answers_the_page_and_the_two_acl_dialogs() {
    let app = app_of(Source::Fixture);
    let (s, v) = json(&app, get("/api/v1/access?state=secured")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["kinds"].as_array().unwrap().len(), 6);
    assert_eq!(v["operations"].as_array().unwrap().len(), 14);
    assert!(v["access"]["auth"]["on"].as_bool().unwrap(), "{v}");
    for u in v["access"]["users"].as_array().unwrap() {
        let keys: Vec<&String> = u.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 5, "{u}");
    }
    let (s, v) = json(
        &app,
        get("/api/v1/access/acls/new?type=group&name=payments-workers&pattern=prefixed&principal=User:payments&op=read&perm=deny"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let call = v["call"].to_string();
    assert!(
        call.contains("CreateAcls")
            && call.contains("GROUP")
            && call.contains("PREFIXED")
            && call.contains("DENY"),
        "{call}"
    );
    let (s, v) = json(
        &app,
        post(
            "/api/v1/access/acls/new",
            &serde_json::json!({"type": "topic", "principal": "User:x"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["outcome"], "invalid", "{v}");
    assert_eq!(v["form"]["error"][1], "Type the resource's name.");
    let (s, v) = json(&app, get("/api/v1/access/acls/delete?type=topic")).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("BAD_REQUEST"))
    );
}

/// CON-1312: a write of Settings, Access or the Lab needs the console's header and the same-origin proof.
#[tokio::test]
async fn a_write_of_settings_access_or_the_lab_needs_the_same_origin_proof() {
    let app = app_of(Source::Fixture);
    for path in [
        "/api/v1/settings/edit",
        "/api/v1/access/acls/new",
        "/api/v1/access/acls/delete",
        "/api/v1/lab/rules",
        "/api/v1/lab/rules/1/remove",
        "/api/v1/lab/once/drop",
        "/api/v1/lab/compact",
    ] {
        let foreign = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, HOST)
            .header("x-console", "1")
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::from("{}"))
            .unwrap();
        let (s, v) = json(&app, foreign).await;
        assert_eq!(
            (s, v["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("FORBIDDEN")),
            "{path}"
        );
        let bare = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, HOST)
            .body(Body::from("{}"))
            .unwrap();
        let (s, _) = json(&app, bare).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }
    // Where the Lab is not installed, every Lab write says so.
    let (s, v) = json(
        &app,
        post("/api/v1/lab/rules", &serde_json::json!({"kind": "slow"})),
    )
    .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("LAB_OFF"))
    );
    let (s, v) = json(&app, get("/api/v1/lab")).await;
    assert_eq!(
        (s, v["home"]["off"].as_bool()),
        (StatusCode::OK, Some(true))
    );
}

/// CON-1320 to CON-1324: the Lab over a real broker: the form, the server's preview, a FAIL rule on Produce answers the client
/// with its code and the rule's page lists the hit with what the rule did; switch off, remove, and a second remove is NOT_FOUND.
#[tokio::test]
async fn the_lab_api_previews_makes_switches_and_removes_a_rule_over_a_real_broker() {
    let l = lab_world("r4-api").await;
    let live = Live::new(l.w.backend.clone(), l.w.facts.clone())
        .with_recorder(Arc::clone(&l.rec))
        .with_lab(Arc::clone(l.lab.table()));
    let app = app_of(Source::Live(Box::new(live)));
    let (s, v) = json(&app, get("/api/v1/lab/form?kind=fail")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["fields"]["code"], "7", "{v}");
    assert!(
        v["apis"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["value"] == "OffsetCommit"),
        "{v}"
    );
    assert_eq!(v["codes"].as_array().unwrap().len(), 3);
    let fields = serde_json::json!({
        "api": "Produce", "topic": "orders", "partitions": "0", "client": "r4-client",
        "share": "every", "ends": "5", "code": "6"
    });
    let (_, v) = json(
        &app,
        post(
            "/api/v1/lab/rules",
            &serde_json::json!({"kind": "fail", "fields": fields, "check": true}),
        ),
    )
    .await;
    assert_eq!(v["outcome"], "checked", "{v}");
    assert_eq!(
        v["form"]["preview"]["sentence"],
        "Answers every Produce request to orders-0 from r4-client with NOT_LEADER_OR_FOLLOWER."
    );
    let (_, v) = json(
        &app,
        post(
            "/api/v1/lab/rules",
            &serde_json::json!({"kind": "fail", "fields": fields}),
        ),
    )
    .await;
    assert_eq!(v["outcome"], "made", "{v}");
    let id = v["id"].as_u64().unwrap();
    // The client asks; the Lab answers with the code and the broker is never asked.
    let mut c = Raw::connect(&l.w, "r4-client").await;
    let a = c
        .try_call(ApiKey::Produce, 9, &produce_req("orders"))
        .await
        .unwrap();
    let r: ProduceResponse = body(ApiKey::Produce, 9, a);
    assert_eq!(r.responses[0].partition_responses[0].error_code, 6);
    let (s, v) = json(&app, get(&format!("/api/v1/lab/rules/{id}"))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["rule"]["hits"], 1, "{v}");
    assert_eq!(v["hits"][0]["code_name"], "NOT_LEADER_OR_FOLLOWER", "{v}");
    assert!(
        v["hits"][0]["did"]
            .as_str()
            .unwrap()
            .starts_with("answered NOT_LEADER_OR_FOLLOWER"),
        "{v}"
    );
    // Switch off, then remove; a rule not in force is NOT_FOUND.
    let (s, _) = json(
        &app,
        post(
            &format!("/api/v1/lab/rules/{id}/enabled"),
            &serde_json::json!({"on": false}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = json(&app, get("/api/v1/lab")).await;
    assert_eq!(v["home"]["active"][0]["enabled"], false, "{v}");
    let (s, _) = json(
        &app,
        post(
            &format!("/api/v1/lab/rules/{id}/remove"),
            &serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = json(
        &app,
        post(
            &format!("/api/v1/lab/rules/{id}/remove"),
            &serde_json::json!({}),
        ),
    )
    .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("NOT_FOUND"))
    );
    let (_, v) = json(&app, get("/api/v1/lab")).await;
    assert_eq!(v["home"]["ended"][0]["ended"], "cleared", "{v}");
    // A wrong field: the engine's sentence under it.
    let (_, v) = json(
        &app,
        post(
            "/api/v1/lab/rules",
            &serde_json::json!({"kind": "slow", "fields": {"delay": "0"}}),
        ),
    )
    .await;
    assert_eq!(v["outcome"], "invalid", "{v}");
    assert!(
        v["form"]["errors"]
            .to_string()
            .contains("Type a delay from 1 to 60,000 ms."),
        "{v}"
    );
    // The rehearsals' dialogs and Compact now answer for a live broker.
    let (s, v) = json(&app, get("/api/v1/lab/once/rebalance")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["nothing"], "No group has a member right now.", "{v}");
    let (s, v) = json(&app, get("/api/v1/lab/compact?topic=orders&partition=0")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        v["blocked"].as_str().unwrap().contains("does not compact"),
        "{v}"
    );
    l.w.stop().await;
}

/// CON-1325: a rule's form takes several APIs, several topics, partitions and a group, and says what is wrong with each.
#[test]
fn a_rule_form_takes_several_apis_topics_partitions_and_a_group() {
    let f = LabFields {
        api: "produce, Fetch".into(),
        topic: "orders,payments.*".into(),
        partitions: "2,0,2".into(),
        group: "billing".into(),
        delay: "250".into(),
        share: "p50".into(),
        ends: "10".into(),
        ..LabFields::default()
    };
    let s = spec_of(LabKind::Slow, &f).unwrap();
    assert_eq!(s.matcher.apis, vec![ApiKey::Produce, ApiKey::Fetch]);
    assert_eq!(s.matcher.topics.len(), 2);
    assert_eq!(s.matcher.partitions, vec![0, 2]);
    assert_eq!(
        s.matcher.group.as_ref().map(|g| g.as_str()),
        Some("billing")
    );
    assert_eq!(s.share, Share::Percent(50));
    assert_eq!(
        s.effect,
        Effect::Slow {
            delay_ms: 250,
            jitter_ms: 0
        }
    );
    let bad = LabFields {
        api: String::new(),
        partitions: "x".into(),
        ..f.clone()
    };
    let e = spec_of(LabKind::Slow, &bad).unwrap_err();
    let names: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        names.contains(&"api") && names.contains(&"partitions"),
        "{names:?}"
    );
    let orphan = LabFields {
        topic: String::new(),
        ..f
    };
    let e = spec_of(LabKind::Slow, &orphan).unwrap_err();
    assert_eq!(
        e[0].1,
        "Partitions belong to a topic: choose the topic first."
    );
}
