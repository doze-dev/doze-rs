//! The fields serde writes through a module of `api::settings` (`#[serde(with = "..")]`: a config's source as words, a
//! refusal as Kafka's name, number and words) are sent as the TypeScript types in `web/src/api/types` say, over a real broker:
//! the keys of each answer are the keys the type declares, and the source is a string.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_access;
mod support_api;
mod support_write;

use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_auth::Principal;
use kafka_broker::Caller;
use serde_json::Value;
use support::{HOST, send};
use support_access::seeded;
use support_api::{api, get};

/// The top-level keys of `export type Name = { .. }` in the generated file.
fn ts_keys(name: &str) -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/api/types")
        .join(format!("{name}.ts"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    // Comments out, then the body of the object type.
    let mut plain = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("/*") {
        plain.push_str(&rest[..at]);
        rest = &rest[at + rest[at..].find("*/").unwrap() + 2..];
    }
    plain.push_str(rest);
    let body = &plain[plain.find("= {").unwrap() + 3..plain.rfind('}').unwrap()];
    let (mut depth, mut start, mut keys) = (0_i32, 0, BTreeSet::new());
    for (i, c) in body.char_indices() {
        match c {
            '{' | '[' | '<' | '(' => depth += 1,
            '}' | ']' | '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                let part = body[start..i].trim();
                if let Some((k, _)) = part.split_once(':') {
                    keys.insert(k.trim().to_owned());
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    keys
}

fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object()
        .unwrap_or_else(|| panic!("not an object: {v}"))
        .keys()
        .cloned()
        .collect()
}

#[tokio::test]
async fn a_source_and_a_refusal_are_sent_as_the_typescript_types_say() {
    let w = seeded("api-types").await;
    let app = api(&w);
    // SettingsConfigRow.source: `string`, the words of the table, not the enum.
    let (s, page) = get(&app, "/api/v1/settings").await;
    assert_eq!(s, StatusCode::OK);
    let rows = page["settings"]["configs"]["rows"].as_array().unwrap();
    assert!(!rows.is_empty(), "{page}");
    for r in rows {
        assert_eq!(keys(r), ts_keys("SettingsConfigRow"), "{r}");
        let source = r["source"].as_str().unwrap_or_else(|| panic!("{r}"));
        assert!(
            [
                "Dynamic broker",
                "Dynamic default broker",
                "Static broker",
                "Default",
                "Dynamic topic",
                "Other"
            ]
            .contains(&source),
            "the words, not the enum's own name: {source}"
        );
    }
    // SettingsRefused.refusal: a KafkaRefusal (a value the broker does not take, checked only).
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/settings/edit")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"key":"log.segment.bytes","was":"log.segment.bytes","scope":"all","value":"1","check":true}"#,
        ))
        .unwrap();
    let (res, text) = send(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK, "{text}");
    let out: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(out["outcome"], "refused", "{out}");
    assert_eq!(keys(&out["refused"]), ts_keys("SettingsRefused"), "{out}");
    let refusal = &out["refused"]["refusal"];
    assert_eq!(keys(refusal), ts_keys("KafkaRefusal"), "{refusal}");
    assert_eq!(refusal["code"], 42, "{refusal}");
    assert_eq!(refusal["name"], "INVALID_REQUEST", "{refusal}");
    // AccessAcls.refused: `KafkaRefusal | null`, null for the console and a refusal for a caller DescribeAcls refuses.
    let (_, access) = get(&app, "/api/v1/access").await;
    assert_eq!(
        keys(&access["access"]["acls"]),
        ts_keys("AccessAcls"),
        "{access}"
    );
    assert!(access["access"]["acls"]["refused"].is_null(), "{access}");
    w.stop().await;
    let s = support_access::secured("api-types-acl", false).await;
    let nobody = Caller::new(
        Principal::user("nobody"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let app = support_access::router_with(
        s.w.facts.clone(),
        support_access::start(false),
        s.w.backend.acting_as(nobody),
    );
    let (status, access) = get(&app, "/api/v1/access").await;
    assert_eq!(status, StatusCode::OK, "{access}");
    let refused = &access["access"]["acls"]["refused"];
    assert_eq!(keys(refused), ts_keys("KafkaRefusal"), "{access}");
    assert_eq!(refused["name"], "CLUSTER_AUTHORIZATION_FAILED", "{refused}");
    assert_eq!(refused["code"], 31, "{refused}");
}
