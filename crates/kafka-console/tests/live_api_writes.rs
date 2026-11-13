//! Slice R1b: the JSON API of the topic writes over a real broker in this process. Every write is the
//! protocol request the classic forms send; a refusal is `{refused, status, code, message, detail}` with Kafka's error name and the broker's
//! sentence; the form GETs change nothing; a POST without the same-origin proof is refused.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_write;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use support::{HOST, send};
use support_api::{api, get, post};
use support_write::world;

/// CON-1150, CON-1151: Create topic: the form view says what is wrong (taken, colliding) before anything is sent; Check only
/// makes nothing; Create makes the topic; a name taken since is Kafka's TOPIC_ALREADY_EXISTS with its sentence.
#[tokio::test]
async fn create_topic_checks_makes_and_is_refused_in_kafkas_words() {
    let w = world("api-create", |_, _| {}).await;
    w.create_topic("payments.settled", 1, &[]).await;
    let app = api(&w);
    let (s, v) = get(
        &app,
        "/api/v1/create-topic?step=review&name=payments_settled&partitions=1",
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["view"]["step"], "name");
    assert_eq!(
        v["view"]["errors"]["name"],
        "This name collides with the topic payments.settled."
    );
    let fields = [
        ("name", "fresh.one"),
        ("partitions", "3"),
        ("template", "compact"),
        ("mode", "check"),
    ];
    let (s, v) = post(&app, "/api/v1/create-topic", &fields).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["checked"], true);
    assert!(
        !w.topics().await.iter().any(|(n, _)| n == "fresh.one"),
        "Check only makes nothing"
    );
    let mut make = fields;
    make[3] = ("mode", "create");
    let (s, v) = post(&app, "/api/v1/create-topic", &make).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["created"]["name"], "fresh.one");
    assert_eq!(v["created"]["partitions"], 3);
    assert_eq!(v["created"]["cleanup"], "compact");
    assert_eq!(
        w.config("fresh.one", "cleanup.policy").await.as_deref(),
        Some("compact")
    );
    let (s, v) = post(&app, "/api/v1/create-topic", &make).await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(422)),
        "{v}"
    );
    assert_eq!(v["code"], "TOPIC_ALREADY_EXISTS");
    assert_eq!(v["detail"]["refusal"]["code"], 36);
    assert_eq!(v["message"], v["detail"]["refusal"]["message"]);
    assert_eq!(v["detail"]["existing"], "fresh.one");
    // A number that is not one: the form again, 400.
    let (s, v) = post(
        &app,
        "/api/v1/create-topic",
        &[("name", "x"), ("partitions", "three"), ("mode", "create")],
    )
    .await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(400)),
        "{v}"
    );
    assert_eq!(v["code"], "INVALID");
    w.stop().await;
}

/// CON-1152, CON-1153: Produce: the form GET draws the call for what was typed and writes nothing; a keyed record lands
/// at the offset the log chose; a keyless one to a compacted topic is INVALID_RECORD with the key field's sentence.
#[tokio::test]
async fn produce_writes_one_record_and_a_keyless_one_to_a_compacted_topic_is_refused() {
    let w = world("api-produce", |_, _| {}).await;
    w.create_topic("orders", 3, &[]).await;
    w.create_topic("state", 1, &[("cleanup.policy", "compact")])
        .await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/topics/orders/produce?partition=2&key=k1&value=v1&hn0=content-type&hv0=json&acks=1").await;
    assert_eq!(s, StatusCode::OK);
    let call: String = v["view"]["call"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|line| {
            line.as_array()
                .unwrap()
                .iter()
                .map(|t| t[1].as_str().unwrap().to_owned())
                .chain(["\n".to_owned()])
        })
        .collect();
    assert!(
        call.contains("index: 2") && call.contains("content-type"),
        "{call}"
    );
    assert_eq!(w.offset("orders", 2, -1).await, 0, "a GET writes nothing");
    let fields = [
        ("partition", "2"),
        ("key", "k1"),
        ("value", "v1"),
        ("acks", "-1"),
        ("hn0", "content-type"),
        ("hv0", "json"),
    ];
    let (s, v) = post(&app, "/api/v1/topics/orders/produce", &fields).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["produced"]["partition"], 2);
    assert_eq!(v["produced"]["offset"], 0);
    assert_eq!(w.offset("orders", 2, -1).await, 1);
    let (s, v) = post(
        &app,
        "/api/v1/topics/state/produce",
        &[("value", "v"), ("acks", "-1")],
    )
    .await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(422)),
        "{v}"
    );
    assert_eq!(v["code"], "INVALID_RECORD");
    assert_eq!(v["detail"]["refused"]["refusal"]["code"], 87);
    assert_eq!(
        v["detail"]["refused"]["view"]["errors"]["key"],
        "A compacted topic needs a key on every record."
    );
    assert_eq!(w.offset("state", 0, -1).await, 0);
    let (s, v) = post(
        &app,
        "/api/v1/topics/orders/produce",
        &[("partition", "9"), ("acks", "-1")],
    )
    .await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(400)),
        "{v}"
    );
    let (s, _) = get(&app, "/api/v1/topics/nothing.here/produce").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    w.stop().await;
}

/// CON-1154, CON-1155, CON-1156: Add partitions raises the count and fewer is INVALID_PARTITIONS; Delete records moves the
/// log start and is POLICY_VIOLATION on a topic that only compacts; Delete topic needs the name typed, then the topic is gone.
#[tokio::test]
async fn add_partitions_delete_records_and_delete_topic() {
    let w = world("api-admin", |_, _| {}).await;
    w.create_topic("grow", 1, &[]).await;
    w.create_topic("state", 1, &[("cleanup.policy", "compact")])
        .await;
    let app = api(&w);
    let (_, v) = get(&app, "/api/v1/topics/grow/add-partitions").await;
    assert_eq!(v["view"]["count"], "2");
    assert_eq!(v["button"], "Add 1 partition");
    let (s, v) = post(
        &app,
        "/api/v1/topics/grow/add-partitions",
        &[("count", "3")],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["from"].as_i64(), v["to"].as_i64()), (Some(1), Some(3)));
    let (s, v) = post(
        &app,
        "/api/v1/topics/grow/add-partitions",
        &[("count", "2")],
    )
    .await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(422)),
        "{v}"
    );
    assert_eq!(v["code"], "INVALID_PARTITIONS");
    for i in 0..4 {
        let value = format!("v{i}");
        let (s, _) = post(
            &app,
            "/api/v1/topics/grow/produce",
            &[
                ("partition", "1"),
                ("key", "k"),
                ("value", &value),
                ("acks", "-1"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
    }
    let (_, v) = get(
        &app,
        "/api/v1/topics/grow/delete-records?partition=1&offset=3",
    )
    .await;
    assert_eq!(v["button"], "Delete 3 records");
    assert_eq!(v["partitions"], serde_json::json!([0, 1, 2]));
    let (s, v) = post(
        &app,
        "/api/v1/topics/grow/delete-records",
        &[("partition", "1"), ("offset", "3")],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["low_watermark"], 3);
    assert_eq!(w.offset("grow", 1, -2).await, 3);
    let (s, _) = post(
        &app,
        "/api/v1/topics/state/produce",
        &[("key", "a"), ("value", "1"), ("acks", "-1")],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = post(
        &app,
        "/api/v1/topics/state/delete-records",
        &[("partition", "0"), ("offset", "1")],
    )
    .await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(422)),
        "{v}"
    );
    assert_eq!(v["code"], "POLICY_VIOLATION");
    let (s, v) = post(&app, "/api/v1/topics/grow/delete", &[("confirm", "gro")]).await;
    assert_eq!(
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(400)),
        "{v}"
    );
    assert_eq!(
        v["detail"]["view"]["error"],
        "Type the name of the topic to confirm."
    );
    let (s, v) = post(&app, "/api/v1/topics/grow/delete", &[("confirm", "grow")]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["deleted"]["name"], "grow");
    assert!(!w.topics().await.iter().any(|(n, _)| n == "grow"));
    w.stop().await;
}

/// CON-1160: a write without the same-origin proof is refused before anything is sent.
#[tokio::test]
async fn a_write_without_the_same_origin_proof_is_refused() {
    let w = world("api-proof", |_, _| {}).await;
    w.create_topic("t", 1, &[]).await;
    let app = api(&w);
    let before = w.config("t", "retention.ms").await;
    for path in [
        "/api/v1/create-topic",
        "/api/v1/topics/t/produce",
        "/api/v1/topics/t/add-partitions",
        "/api/v1/topics/t/delete-records",
        "/api/v1/topics/t/delete",
        "/api/v1/topics/t/config/edit",
        "/api/v1/topics/t/config/reset",
    ] {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, HOST)
            .header("x-console", "1")
            .header(header::ORIGIN, "http://evil.example")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "name=x&confirm=t&count=5&key=retention.ms&value=1",
            ))
            .unwrap();
        let (res, text) = send(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}: {text}");
        assert!(text.contains("\"code\":\"FORBIDDEN\""), "{text}");
    }
    assert!(
        w.topics().await.iter().any(|(n, _)| n == "t"),
        "nothing was deleted"
    );
    assert_eq!(w.config("t", "retention.ms").await, before);
    w.stop().await;
}
