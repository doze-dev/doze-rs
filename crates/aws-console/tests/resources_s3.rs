//! A bucket's pages against a real S3 on a real bus: its header (the creation date, the sentence, what it notifies), what each
//! of its tabs may ask of S3 (every call is on this bucket and no other), and a copy between buckets.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
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
use deep::{ok, op};
use serde_json::{Value, json};
use support::{HOST, send as raw};
use world::{World, field, world};

const BASE: &str = "/api/v1/resources/s3/buckets";

/// An object put through the console's upload address, as its page sends one.
async fn put(w: &World, bucket: &str, key: &str, kind: &str) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("{BASE}/{bucket}/object?key={}", world::encode(key)))
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, kind)
        .header(header::CONTENT_LENGTH, 3)
        .body(Body::from("abc"))
        .unwrap();
    let (res, text) = raw(&w.app, req).await;
    assert_eq!(res.status(), 201, "{text}");
}

#[tokio::test]
async fn a_bucket_says_when_it_was_made_what_it_holds_and_what_it_notifies() {
    let w = world("s3-header", Mode::Soft, &[]);
    w.app("s3", "CreateBucket", &json!({"Bucket": "images"}))
        .await;
    w.app("s3", "CreateBucket", &json!({"Bucket": "images-old"}))
        .await;
    let (_, d) = w.get(&format!("{BASE}/images")).await;
    assert_eq!(d["sentence"], "images is empty.");
    // The creation date is the bucket's own, not the first one listed with its name as a prefix.
    let created = field(&d, "Created");
    assert_eq!(created["format"], "time");
    let (_, list) = w.get(BASE).await;
    let row = |n: &str| {
        list["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == n)
            .unwrap()
            .clone()
    };
    let listed = |n: &str| row(n)["cells"][1].clone();
    assert_eq!(created["value"], listed("images"));
    assert_ne!(listed("images"), Value::Null);
    assert_eq!(field(&d, "Region")["value"], "us-east-1");
    assert_eq!(field(&d, "Versioning")["value"], "Disabled");
    assert_eq!(d["tags"], true);
    assert_eq!(
        d["deep"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["objects", "properties", "permissions", "management"]
    );

    w.app(
        "s3",
        "PutBucketVersioning",
        &json!({"Bucket": "images", "VersioningConfiguration": {"Status": "Enabled"}}),
    )
    .await;
    put(&w, "images", "a.jpg", "image/jpeg").await;
    put(&w, "images", "banners/b.jpg", "image/jpeg").await;
    w.app(
        "s3",
        "PutBucketNotificationConfiguration",
        &json!({"Bucket": "images", "NotificationConfiguration": {
            "LambdaFunctionConfigurations": [{"Id": "resize", "LambdaFunctionArn": "arn:aws:lambda:us-east-1:000000000000:function:resize-image", "Events": ["s3:ObjectCreated:*"]}]}}),
    )
    .await;
    let (_, d) = w.get(&format!("{BASE}/images")).await;
    assert_eq!(
        d["sentence"],
        "images holds 1 object and 1 folder at the top level, with versioning on. New objects notify resize-image."
    );
    assert_eq!(field(&d, "Versioning")["value"], "Enabled");
    // The strip of what it is wired to names the function, linked.
    let wired = d["wired"].as_array().unwrap();
    assert_eq!(wired.len(), 1, "{wired:?}");
    assert_eq!(wired[0]["label"], "Notifies Lambda function");
    assert_eq!(
        wired[0]["to"],
        json!({"service": "lambda", "kind": "functions", "name": "resize-image"})
    );
}

#[tokio::test]
async fn the_list_of_buckets_says_how_many_and_where() {
    let w = world("s3-list", Mode::Soft, &[]);
    let (_, empty) = w.get(BASE).await;
    assert_eq!(empty["sentence"], Value::Null);
    for b in ["alpha", "beta"] {
        w.app("s3", "CreateBucket", &json!({"Bucket": b})).await;
    }
    let (_, list) = w.get(BASE).await;
    assert_eq!(
        list["sentence"],
        "2 buckets in this account, all in us-east-1."
    );
}

#[tokio::test]
async fn each_tab_of_a_bucket_asks_only_what_it_lists_and_only_of_its_own_bucket() {
    let w = world("s3-tabs", Mode::Soft, &[]);
    for b in ["mine", "theirs"] {
        w.app("s3", "CreateBucket", &json!({"Bucket": b})).await;
    }
    let mine = format!("{BASE}/mine");
    // Permissions: CORS, a policy, the public access block and the ownership, written and read back.
    let cors = json!({"CORSRules": [{"AllowedMethods": ["GET"], "AllowedOrigins": ["http://localhost:3000"], "MaxAgeSeconds": 3000}]});
    ok(
        &w,
        &mine,
        "PutBucketCors",
        &json!({"CORSConfiguration": cors}),
    )
    .await;
    let got = ok(&w, &mine, "GetBucketCors", &json!({})).await;
    assert_eq!(
        got["CORSRules"][0]["AllowedOrigins"][0],
        "http://localhost:3000"
    );
    let blocked = ok(&w, &mine, "GetPublicAccessBlock", &json!({})).await;
    assert_eq!(
        blocked["PublicAccessBlockConfiguration"]["BlockPublicPolicy"],
        true
    );
    ok(
        &w,
        &mine,
        "PutBucketOwnershipControls",
        &json!({"OwnershipControls": {"Rules": [{"ObjectOwnership": "BucketOwnerPreferred"}]}}),
    )
    .await;
    let own = ok(&w, &mine, "GetBucketOwnershipControls", &json!({})).await;
    assert_eq!(
        own["OwnershipControls"]["Rules"][0]["ObjectOwnership"],
        "BucketOwnerPreferred"
    );
    let acl = ok(&w, &mine, "GetBucketAcl", &json!({})).await;
    assert_eq!(acl["Grants"][0]["Permission"], "FULL_CONTROL");
    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"s3:GetObject","Resource":"arn:aws:s3:::mine/*"}]}"#;
    ok(&w, &mine, "PutBucketPolicy", &json!({"Policy": policy})).await;
    let read = ok(&w, &mine, "GetBucketPolicy", &json!({})).await;
    assert_eq!(read["Policy"], policy);
    ok(&w, &mine, "DeleteBucketPolicy", &json!({})).await;
    let (status, gone) = op(&w, &mine, "GetBucketPolicy", &json!({})).await;
    assert_eq!(
        (status, gone["code"].as_str()),
        (404, Some("NoSuchBucketPolicy")),
        "{gone}"
    );
    // Properties: encryption, the website, versioning.
    ok(
        &w,
        &mine,
        "PutBucketEncryption",
        &json!({"ServerSideEncryptionConfiguration": {"Rules": [{"ApplyServerSideEncryptionByDefault": {"SSEAlgorithm": "AES256"}}]}}),
    )
    .await;
    let enc = ok(&w, &mine, "GetBucketEncryption", &json!({})).await;
    assert_eq!(
        enc["ServerSideEncryptionConfiguration"]["Rules"][0]["ApplyServerSideEncryptionByDefault"]
            ["SSEAlgorithm"],
        "AES256"
    );
    ok(
        &w,
        &mine,
        "PutBucketWebsite",
        &json!({"WebsiteConfiguration": {"IndexDocument": {"Suffix": "index.html"}}}),
    )
    .await;
    let site = ok(&w, &mine, "GetBucketWebsite", &json!({})).await;
    assert_eq!(site["IndexDocument"]["Suffix"], "index.html");
    ok(&w, &mine, "DeleteBucketWebsite", &json!({})).await;
    ok(
        &w,
        &mine,
        "PutBucketVersioning",
        &json!({"VersioningConfiguration": {"Status": "Suspended"}}),
    )
    .await;
    // Management: a lifecycle rule.
    let rules = json!({"Rules": [{"ID": "prune", "Status": "Enabled", "Filter": {"Prefix": "catalog/"}, "NoncurrentVersionExpiration": {"NoncurrentDays": 30}}]});
    ok(
        &w,
        &mine,
        "PutBucketLifecycleConfiguration",
        &json!({"LifecycleConfiguration": rules}),
    )
    .await;
    let read = ok(&w, &mine, "GetBucketLifecycleConfiguration", &json!({})).await;
    assert_eq!(read["Rules"][0]["ID"], "prune");
    ok(&w, &mine, "DeleteBucketLifecycle", &json!({})).await;

    // Whatever the page names, the bucket is this one: the other bucket's settings are untouched.
    ok(
        &w,
        &mine,
        "PutBucketCors",
        &json!({"Bucket": "theirs", "CORSConfiguration": cors}),
    )
    .await;
    let (status, none) = op(&w, &format!("{BASE}/theirs"), "GetBucketCors", &json!({})).await;
    assert_eq!(
        (status, none["code"].as_str()),
        (404, Some("NoSuchCORSConfiguration")),
        "{none}"
    );
    // A call no tab lists is not made.
    let (status, refused) = op(&w, &mine, "DeleteBucket", &json!({})).await;
    assert_eq!((status, refused["code"].as_str()), (404, Some("NOT_FOUND")));
    let (status, refused) = op(&w, &mine, "PutBucketAcl", &json!({"ACL": "public-read"})).await;
    assert_eq!((status, refused["code"].as_str()), (404, Some("NOT_FOUND")));
}

#[tokio::test]
async fn a_public_policy_is_refused_while_public_policies_are_blocked_and_the_refusal_passes_through()
 {
    let w = world("s3-public", Mode::Soft, &[]);
    w.app("s3", "CreateBucket", &json!({"Bucket": "site"}))
        .await;
    let public = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::site/*"}]}"#;
    let (status, refused) = op(
        &w,
        &format!("{BASE}/site"),
        "PutBucketPolicy",
        &json!({"Policy": public}),
    )
    .await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(refused["code"], "AccessDenied");
    // Switching that block off lets the same policy in.
    ok(
        &w,
        &format!("{BASE}/site"),
        "PutPublicAccessBlock",
        &json!({"PublicAccessBlockConfiguration": {"BlockPublicAcls": true, "IgnorePublicAcls": true, "BlockPublicPolicy": false, "RestrictPublicBuckets": false}}),
    )
    .await;
    ok(
        &w,
        &format!("{BASE}/site"),
        "PutBucketPolicy",
        &json!({"Policy": public}),
    )
    .await;
}

#[tokio::test]
async fn an_object_is_copied_between_buckets_by_the_kinds_own_operation() {
    let w = world("s3-copy", Mode::Soft, &[]);
    for b in ["from", "dest"] {
        w.app("s3", "CreateBucket", &json!({"Bucket": b})).await;
    }
    put(&w, "from", "a/x.txt", "text/plain").await;
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/op/CopyObject"),
            &json!({"Bucket": "dest", "Key": "copy/x.txt", "CopySource": "from/a/x.txt"}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let head = ok(
        &w,
        &format!("{BASE}/dest"),
        "HeadObject",
        &json!({"Key": "copy/x.txt"}),
    )
    .await;
    assert_eq!(head["ContentType"], "text/plain");
    // The source is still there, and a copy is the console's own call in Live.
    ok(
        &w,
        &format!("{BASE}/from"),
        "HeadObject",
        &json!({"Key": "a/x.txt"}),
    )
    .await;
    assert!(
        w.live().iter().any(|l| l == "console s3 CopyObject"),
        "{:?}",
        w.live()
    );
    // A kind's other operations are not reachable at its address.
    let (status, refused) = w
        .send(
            "POST",
            &format!("{BASE}/op/DeleteBucket"),
            &json!({"Bucket": "from"}),
        )
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );
}
