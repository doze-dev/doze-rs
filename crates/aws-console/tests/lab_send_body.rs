//! Send a call with an object's body: S3 `PutObject` takes a small text body (the input's `Body`, at most 64 KiB) and puts it as the console's
//! own upload does, over the bus with the bytes; `UploadPart` is refused with its reason; and the operation says which it is, so the page can.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
// Each test binary uses some of the harness.
#[allow(dead_code)]
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use world::{World, world};

async fn bucket(name: &str) -> World {
    let w = world(name, Mode::Soft, &[]);
    w.app("s3", "CreateBucket", &json!({"Bucket": "notes"}))
        .await;
    w
}

async fn put(w: &World, input: &Value) -> (u16, Value) {
    w.send("POST", "/api/v1/lab/s3/PutObject", input).await
}

async fn text_of(w: &World, key: &str) -> (u16, String) {
    let get = support::get(&format!(
        "/api/v1/resources/s3/buckets/notes/object?key={}",
        world::encode(key)
    ));
    let (parts, body) = w.app.clone().oneshot(get).await.unwrap().into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        parts.status.as_u16(),
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

async fn count(w: &World) -> usize {
    let listed = w
        .send(
            "POST",
            "/api/v1/lab/s3/ListObjectsV2",
            &json!({"Bucket": "notes"}),
        )
        .await
        .1;
    let output: Value = serde_json::from_str(listed["output"].as_str().unwrap()).unwrap();
    output["Contents"].as_array().map_or(0, Vec::len)
}

#[tokio::test]
async fn a_put_object_from_send_a_call_puts_its_text_body_as_an_upload_does() {
    let w = bucket("lab-body-put").await;
    let (status, answer) = put(
        &w,
        &json!({ "Bucket": "notes", "Key": "a/hello.txt", "ContentType": "text/plain", "Body": "hello, lab\nsecond line" }),
    )
    .await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["error"], Value::Null, "{answer}");
    let output: Value = serde_json::from_str(answer["output"].as_str().unwrap()).unwrap();
    assert!(output["ETag"].as_str().unwrap().len() > 8, "{output}");
    assert_eq!(
        text_of(&w, "a/hello.txt").await,
        (200, "hello, lab\nsecond line".to_owned())
    );
    let head = w
        .send(
            "POST",
            "/api/v1/lab/s3/HeadObject",
            &json!({ "Bucket": "notes", "Key": "a/hello.txt" }),
        )
        .await
        .1;
    let head: Value = serde_json::from_str(head["output"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&head["ContentLength"], &head["ContentType"]),
        (&json!(22), &json!("text/plain"))
    );
    // It is the console's own call, on Live, and it carries the length of what was sent.
    let call = w
        .state
        .recorder
        .get(answer["seq"].as_u64().unwrap())
        .unwrap();
    assert_eq!((&*call.caller, call.action), ("console", "PutObject"));
}

#[tokio::test]
async fn a_put_object_with_no_body_makes_an_empty_object_and_a_body_of_64_kib_goes_through() {
    let w = bucket("lab-body-sizes").await;
    let (status, _) = put(&w, &json!({ "Bucket": "notes", "Key": "empty" })).await;
    assert_eq!(status, 200);
    assert_eq!(text_of(&w, "empty").await, (200, String::new()));
    let most = "x".repeat(64 << 10);
    let (status, answer) = put(
        &w,
        &json!({ "Bucket": "notes", "Key": "most", "Body": most }),
    )
    .await;
    assert_eq!((status, &answer["error"]), (200, &Value::Null), "{answer}");
    assert_eq!(text_of(&w, "most").await.1.len(), 64 << 10);
}

#[tokio::test]
async fn a_body_past_64_kib_or_not_text_is_refused_and_nothing_is_put() {
    let w = bucket("lab-body-refused").await;
    let past = "x".repeat((64 << 10) + 1);
    let (status, refused) = put(
        &w,
        &json!({ "Bucket": "notes", "Key": "big", "Body": past }),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (413, Some("BODY_TOO_LARGE")),
        "{refused}"
    );
    assert!(refused["message"].as_str().unwrap().contains("64 KiB"));
    let (status, refused) = put(
        &w,
        &json!({ "Bucket": "notes", "Key": "list", "Body": [1, 2] }),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("BAD_REQUEST"))
    );
    assert_eq!(count(&w).await, 0, "nothing was put");
}

#[tokio::test]
async fn an_upload_part_is_refused_with_its_reason_and_the_operation_says_what_it_carries() {
    let w = bucket("lab-body-part").await;
    let (status, refused) = w
        .send(
            "POST",
            "/api/v1/lab/s3/UploadPart",
            &json!({ "Bucket": "notes", "Key": "k", "UploadId": "u", "PartNumber": 1, "Body": "x" }),
        )
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("NEEDS_MULTIPART")),
        "{refused}"
    );
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("CreateMultipartUpload")
    );
    for (op, body_max, cannot) in [
        ("PutObject", json!(65_536), false),
        ("UploadPart", Value::Null, true),
        ("GetObject", Value::Null, false),
    ] {
        let (status, v) = w.get(&format!("/api/v1/lab/s3/{op}")).await;
        assert_eq!(status, 200, "{op}: {v}");
        assert_eq!(v["body_max"], body_max, "{op}");
        assert_eq!(v["cannot"].is_string(), cannot, "{op}");
    }
}
