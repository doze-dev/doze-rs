//! The Topics API over a real broker with hostile records and names, and on an ACL-on broker (the rules CON-113 and CON-114
//! that drove the server-rendered pages before R5): every byte a record carries comes back as text with its controls written
//! out and a long one cut, the longest legal name is one path segment, and a principal is refused a read in Kafka's words.
//! The React pages render that text as text (`web/e2e/tests/hostile.spec.ts`).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_log;
mod support_write;

use axum::http::StatusCode;
use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::CreateAclsRequest;
use kafka_wire::messages::create_acls_request::AclCreation;
use serde_json::Value;
use support_api::{api, get};
use support_log::records;
use support_write::{enc, world};

/// Every string anywhere in an answer.
fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// CON-113: a record's key, value, header names and values, and a topic name of every legal character and 249 bytes come
/// back from every Topics answer as text: no bidirectional control, no NUL, no BOM, no line separator (each is written out as
/// `⟨U+…⟩`), a markup payload is the same characters (the page renders text, never markup), and a 100 KB line is cut at 64 KiB
/// with its size said.
#[tokio::test]
async fn hostile_keys_values_headers_and_names_come_back_as_text() {
    let w = world("ah-text", |_, _| {}).await;
    let name = format!("a.b_c-D9{}", "z".repeat(249 - 8));
    w.create_topic(&name, 1, &[]).await;
    let payloads = [
        "</script><script>alert(1)</script>".to_owned(),
        "\"><img src=x onerror=alert(1)>".to_owned(),
        "bidi \u{202e}txet\u{2066} nul \u{0} del \u{7f} bom \u{feff} ls \u{2028}".to_owned(),
        format!("long {}", "L".repeat(100_000)),
    ];
    for p in &payloads {
        let r = Rec {
            key: Some(p.as_bytes().to_vec()),
            value: Some(p.as_bytes().to_vec()),
            headers: vec![(p.as_bytes().to_vec(), p.as_bytes().to_vec())],
            ..Rec::default()
        };
        let b = BatchBuilder::from_records(1_000, &[r]).build();
        assert_eq!(w.produce_batch(&name, 0, b, None).await.0, 0);
    }
    let app = api(&w);
    let e = enc(&name);
    let mut paths = vec![
        "/api/v1/topics".to_owned(),
        format!("/api/v1/topics/{e}"),
        format!("/api/v1/topics/{e}/messages"),
        format!("/api/v1/topics/{e}/messages?partition=0&isolation=read_committed"),
        format!("/api/v1/topics/{e}/log"),
    ];
    for i in 0..payloads.len() {
        paths.push(format!("/api/v1/topics/{e}/records/0/{i}"));
    }
    for path in &paths {
        let (s, v) = get(&app, path).await;
        assert_eq!(s, StatusCode::OK, "{path}");
        let mut all = Vec::new();
        strings(&v, &mut all);
        for s in &all {
            for c in [
                '\u{202e}', '\u{2066}', '\u{0}', '\u{feff}', '\u{2028}', '\u{7f}',
            ] {
                assert!(!s.contains(c), "{path}: {c:?} came through in {s:.80}");
            }
            assert!(s.len() <= 70 * 1024, "{path}: a line of {} bytes", s.len());
        }
        assert!(
            all.iter().any(|s| *s == name),
            "{path}: the 249-byte name is there as it is"
        );
    }
    // The markup is the same characters, as text.
    let (_, r0) = get(&app, &format!("/api/v1/topics/{e}/records/0/1")).await;
    assert_eq!(r0["value"]["text"], payloads[1]);
    assert_eq!(r0["headers"][0]["key"], payloads[1]);
    // The controls are written out.
    let (_, r2) = get(&app, &format!("/api/v1/topics/{e}/records/0/2")).await;
    assert_eq!(
        r2["value"]["text"],
        "bidi ⟨U+202E⟩txet⟨U+2066⟩ nul ⟨U+0000⟩ del ⟨U+007F⟩ bom ⟨U+FEFF⟩ ls ⟨U+2028⟩"
    );
    assert_eq!(r2["key"]["text"], r2["value"]["text"]);
    assert_eq!(r2["headers"][0]["value"]["text"], r2["value"]["text"]);
    // The long line is cut, and says how big it is.
    let (_, r3) = get(&app, &format!("/api/v1/topics/{e}/records/0/3")).await;
    assert_eq!(r3["value"]["bytes"], 100_005);
    assert_eq!(r3["value"]["clipped"], true);
    assert!(r3["value"]["text"].as_str().unwrap().len() <= 64 * 1024);
    // One byte more is no topic.
    let (_, v) = get(&app, &format!("/api/v1/topics/{e}z")).await;
    assert_eq!(v["missing"]["code"], "UNKNOWN_TOPIC_OR_PARTITION", "{v}");
    let (s, _) = get(&app, &format!("/api/v1/topics/{e}z/messages")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    w.stop().await;
}

fn creation(topic: &str, principal: &str, op: i8) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = 2;
    c.resource_name = StrBytes::from_string(topic.to_owned());
    c.resource_pattern_type = 3;
    c.principal = StrBytes::from_string(principal.to_owned());
    c.host = StrBytes::from_static_str("*");
    c.operation = op;
    c.permission_type = 3;
    c
}

fn listed(v: &Value) -> Vec<String> {
    v["view"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect()
}

/// CON-114: on an ACL-on broker a principal that may describe a topic but not read it is listed the topic and refused the
/// read in Kafka's name and number (TOPIC_AUTHORIZATION_FAILED, 29); one that may not describe it is told no such topic; one
/// with no right at all sees an empty broker; the console itself reads everything.
#[tokio::test]
async fn on_an_acl_broker_a_principal_that_may_describe_but_not_read_is_refused_in_kafkas_words() {
    let w = world("ah-acl", |c, _| c.acl_default_allow = false).await;
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("gz.events", 1, &[]).await;
    assert_eq!(w.produce_batch("orders", 0, records(3), None).await.0, 0);
    let mut r = CreateAclsRequest::default();
    r.creations = vec![creation("orders", "User:viewer", 8)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(r)).await else {
        panic!()
    };
    assert_eq!(r.results[0].error_code, 0);
    let as_who = |who: &str| {
        let c = Caller::new(Principal::user(who), Some("127.0.0.1".parse().unwrap()));
        support_write::router_of(&w, w.backend.acting_as(c))
    };
    let viewer = as_who("viewer");
    let (_, list) = get(&viewer, "/api/v1/topics").await;
    assert_eq!(
        listed(&list),
        vec!["orders".to_owned()],
        "only what it may describe"
    );
    let (s, msgs) = get(&viewer, "/api/v1/topics/orders/messages").await;
    assert_eq!(s, StatusCode::OK, "{msgs}");
    let text = msgs.to_string();
    assert!(
        text.contains("The broker refused the read: TOPIC_AUTHORIZATION_FAILED (29)."),
        "refused in Kafka's words: {text:.2000}"
    );
    assert!(
        msgs["view"]["rows"].as_array().is_none_or(Vec::is_empty),
        "no record: {text:.2000}"
    );
    let (_, hidden) = get(&viewer, "/api/v1/topics/gz.events").await;
    let (_, never) = get(&viewer, "/api/v1/topics/never.made").await;
    assert_eq!(
        hidden["missing"]["code"], "UNKNOWN_TOPIC_OR_PARTITION",
        "{hidden}"
    );
    assert_eq!(
        hidden["missing"]["code"], never["missing"]["code"],
        "a topic it may not describe is not there"
    );
    let (s, _) = get(&viewer, "/api/v1/topics/gz.events/messages").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let nobody = as_who("nobody");
    let (_, none) = get(&nobody, "/api/v1/topics").await;
    assert!(listed(&none).is_empty(), "an empty broker: {none}");
    let (_, all) = get(&api(&w), "/api/v1/topics/orders/messages").await;
    assert_eq!(
        all["view"]["rows"].as_array().unwrap().len(),
        3,
        "the console reads everything"
    );
    assert!(!all.to_string().contains("AUTHORIZATION_FAILED"));
    w.stop().await;
}
