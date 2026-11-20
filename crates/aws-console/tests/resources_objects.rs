//! A bucket's objects page against a real S3 on a real bus: an upload through the console (over the 1 MiB of a plain request,
//! under its 25 MiB), a folder, browsing by folder and by page, an object's metadata and tags, a download that is the upload
//! byte for byte, deletes, and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "resources/deep.rs"]
mod deep;
mod support;
// Each test binary uses some of the harness.
#[allow(dead_code)]
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use axum::body::Body;
use axum::http::{Request, header};
use deep::{none, ok, op};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::{HOST, header_of, send as raw};
use tower::ServiceExt;
use world::{World, world};

const BASE: &str = "/api/v1/resources/s3/buckets/product-images";

/// An upload as the page sends it: the file's bytes, its type, the key in the address.
fn upload(key: &str, kind: &str, bytes: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("{BASE}/object?key={}", world::encode(key)))
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, kind)
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .unwrap()
}

/// 2 MiB of bytes that are not all alike.
fn big() -> Vec<u8> {
    (0..(2 << 20))
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect()
}

/// A bucket holding `banners/hero.bin` (2 MiB), `readme.txt` and the folder `empty/`.
async fn bucket(name: &str) -> World {
    let w = world(name, Mode::Soft, &[]);
    w.app("s3", "CreateBucket", &json!({"Bucket": "product-images"}))
        .await;
    let (res, text) = raw(
        &w.app,
        upload("banners/hero.bin", "application/x-test", big()),
    )
    .await;
    assert_eq!(res.status(), 201, "{text}");
    let made: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (made["key"].as_str(), made["size"].as_u64()),
        (Some("banners/hero.bin"), Some(2 << 20))
    );
    raw(
        &w.app,
        upload("readme.txt", "text/plain", b"hello".to_vec()),
    )
    .await;
    let (res, _) = raw(
        &w.app,
        upload("empty/", "application/x-directory", Vec::new()),
    )
    .await;
    assert_eq!(res.status(), 201);
    w
}

#[tokio::test]
async fn a_bucket_is_browsed_by_folder_and_by_page() {
    let w = bucket("deep-s3-browse").await;
    let top = ok(
        &w,
        BASE,
        "ListObjectsV2",
        &json!({"Delimiter": "/", "Prefix": ""}),
    )
    .await;
    let prefixes: Vec<&str> = top["CommonPrefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["Prefix"].as_str().unwrap())
        .collect();
    assert_eq!(prefixes, ["banners/", "empty/"]);
    assert_eq!(top["Contents"][0]["Key"], "readme.txt");
    let paged = ok(&w, BASE, "ListObjectsV2", &json!({"MaxKeys": 1})).await;
    assert_eq!(paged["IsTruncated"], true);
    let token = paged["NextContinuationToken"].clone();
    let next = ok(
        &w,
        BASE,
        "ListObjectsV2",
        &json!({"MaxKeys": 1, "ContinuationToken": token}),
    )
    .await;
    assert_ne!(next["Contents"][0]["Key"], paged["Contents"][0]["Key"]);
    let head = ok(&w, BASE, "HeadObject", &json!({"Key": "banners/hero.bin"})).await;
    assert_eq!(head["ContentType"], "application/x-test");
    assert!(none(
        &ok(&w, BASE, "GetObjectTagging", &json!({"Key": "readme.txt"})).await["TagSet"]
    ));
    let (status, _) = op(&w, BASE, "PutObject", &json!({"Key": "x"})).await;
    assert_eq!(status, 404, "PutObject is the upload's alone");
}

#[tokio::test]
async fn an_object_comes_down_as_it_went_up_and_is_deleted() {
    let w = bucket("deep-s3-down").await;
    let get = support::get(&format!(
        "{BASE}/object?key={}",
        world::encode("banners/hero.bin")
    ));
    let (parts, body) = w.app.clone().oneshot(get).await.unwrap().into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    assert!(
        bytes.as_ref() == big().as_slice(),
        "the download is not the upload"
    );
    let res = axum::http::Response::from_parts(parts, ());
    assert_eq!(res.status(), 200);
    assert_eq!(header_of(&res, "content-type"), "application/x-test");
    assert!(header_of(&res, "content-disposition").contains("filename=\"hero.bin\""));

    let both = json!({"Delete": {"Objects": [{"Key": "readme.txt"}, {"Key": "empty/"}]}});
    assert_eq!(
        ok(&w, BASE, "DeleteObjects", &both).await["Deleted"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    ok(
        &w,
        BASE,
        "DeleteObject",
        &json!({"Key": "banners/hero.bin"}),
    )
    .await;
    assert!(none(
        &ok(&w, BASE, "ListObjectsV2", &json!({})).await["Contents"]
    ));
}

#[tokio::test]
async fn an_upload_over_the_cap_and_a_missing_object_are_refused() {
    let w = bucket("deep-s3-refused").await;
    let mut over = upload("huge.bin", "application/octet-stream", Vec::new());
    over.headers_mut()
        .insert(header::CONTENT_LENGTH, (26 << 20).into());
    let (res, _) = raw(&w.app, over).await;
    assert_eq!(res.status(), 413);
    let (status, missing) = w.get(&format!("{BASE}/object?key=nope")).await;
    assert_eq!((status, missing["code"].as_str()), (404, Some("NoSuchKey")));
    let (status, _) = w.get(&format!("{BASE}/object")).await;
    assert_eq!(status, 400);
    let (status, _) = w
        .get("/api/v1/resources/sqs/queues/orders/object?key=x")
        .await;
    assert_eq!(status, 404);
}
