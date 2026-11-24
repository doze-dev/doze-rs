//! The region a request is for, and the index that leaves a sleeping service asleep: the console's calls on the services' bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use support::{get, json as answer, keys, ts_keys};
use world::{world, write};

#[tokio::test]
async fn a_request_is_for_the_region_it_names_and_only_a_region_of_aws_is_one() {
    let w = world("regions", Mode::Soft, &["sqs"]);
    let in_region = |mut req: axum::http::Request<axum::body::Body>, region: &str| {
        req.headers_mut()
            .insert("x-console-region", region.parse().unwrap());
        req
    };
    // A queue made in Ireland is there, and is not the default region's.
    let made = write(
        "POST",
        "/api/v1/resources/sqs/queues",
        &json!({"QueueName": "ie-only"}),
    );
    let (status, body) = answer(&w.app, in_region(made, "eu-west-1")).await;
    assert_eq!(status, 201, "{body}");
    let names = |list: &Value| {
        list["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let (_, home) = w.get("/api/v1/resources/sqs/queues").await;
    assert!(!names(&home).contains(&"ie-only".to_owned()), "{home}");
    let (_, ireland) = answer(
        &w.app,
        in_region(get("/api/v1/resources/sqs/queues"), "eu-west-1"),
    )
    .await;
    assert!(names(&ireland).contains(&"ie-only".to_owned()), "{ireland}");
    // A name that is not a region of AWS is refused, whatever its spelling.
    let (status, refused) = answer(
        &w.app,
        in_region(get("/api/v1/resources/sqs/queues"), "not-a-region-9"),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("UNKNOWN_REGION"))
    );
    // The switcher's list: every region of AWS, the default first, the one that holds the queue before the rest.
    let (status, list) = w.get("/api/v1/regions").await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(keys(&list), ts_keys("RegionsView"));
    let codes: Vec<&str> = list["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert_eq!(list["default"], "us-east-1");
    assert_eq!(codes[0], "us-east-1");
    assert!(
        codes.len() >= 30 && !codes.contains(&"not-a-region-9"),
        "{codes:?}"
    );
    assert_eq!(keys(&list["regions"][0]), ts_keys("RegionRow"));
    let row = |c: &str| {
        list["regions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["code"] == c)
            .unwrap()
            .clone()
    };
    assert_eq!(row("eu-west-1")["has_data"], true, "{list}");
    assert_eq!(row("ap-south-1")["has_data"], false);
}

#[tokio::test]
async fn the_index_can_leave_a_sleeping_service_asleep_and_uncounted() {
    // SQS is called (the queue is made through the console), KMS is not: it is asleep, and listing its keys would wake it.
    let w = world("index-awake", Mode::Soft, &["sqs", "kms"]);
    w.send(
        "POST",
        "/api/v1/resources/sqs/queues",
        &json!({"QueueName": "a"}),
    )
    .await;
    let svc = |index: &Value, n: &str| {
        index["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == n)
            .unwrap()
            .clone()
    };
    let before = w.live().len();
    let (status, awake) = w.get("/api/v1/resources?counts=awake").await;
    assert_eq!(status, 200, "{awake}");
    // The awake one is counted, the sleeping one is neither counted nor called, and it says why.
    let (sqs, kms) = (svc(&awake, "sqs"), svc(&awake, "kms"));
    assert_eq!(
        (&sqs["asleep"], &sqs["kinds"][0]["count"]),
        (&json!(false), &json!(1))
    );
    assert_eq!(
        (&kms["asleep"], &kms["kinds"][0]["count"]),
        (&json!(true), &Value::Null)
    );
    assert!(
        !w.live()[before..].iter().any(|c| c.contains(" kms ")),
        "counting the awake services called KMS"
    );
    // A service this run does not serve is not asleep (it is not served), and the other counts leave nothing asleep.
    assert_eq!(svc(&awake, "sns")["asleep"], false);
    let (_, all) = w.get("/api/v1/resources").await;
    assert_eq!(svc(&all, "kms")["asleep"], false);
    assert_ne!(svc(&all, "kms")["kinds"][0]["count"], Value::Null);
    let (_, bare) = w.get("/api/v1/resources?counts=no").await;
    assert_eq!(svc(&bare, "kms")["asleep"], false);
}
