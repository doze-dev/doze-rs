//! A folder the Go doze-aws module left, carried over: set aside at provision, replayed at converge, once.
//!
//! Needs the `aws` CLI; without it the test is skipped unless `DOZE_ACCEPTANCE` is set.
#![cfg(feature = "module")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod module_support;

use std::path::PathBuf;
use std::time::Duration;

use doze_module_test::Booted;
use module_support::{aws, converge, harness, have_cli, queue_urls};

/// An instance folder the Go doze-aws module left (tests/fixtures/go-data, written by the Go server through the aws CLI:
/// a versioned bucket with two versions and a delete marker, an object with metadata, a cache header and tags, a bucket
/// and queues doze.hcl does not declare, a redrive, string and binary message attributes, a message in flight and a
/// FIFO queue) is set aside at provision and carried over at converge, once.
#[tokio::test(flavor = "current_thread")]
async fn the_go_modules_data_is_carried_over() {
    if !have_cli() {
        return;
    }
    let seed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/go-data");
    let hcl = "aws \"cloud\" {\n  bucket \"receipts\" {\n    versioning = true\n  }\n  bucket \"product-images\" {}\n  queue \"orders\" {}\n}\n";
    let b = harness()
        .boot_seeded(hcl, doze_module_test::Boot::default(), &seed)
        .await
        .unwrap();
    let data = PathBuf::from(&b.instance.data_dir);
    assert!(data.join("go-data/s3/meta.bolt").is_file() && !data.join("s3/meta.bolt").exists());
    converge(&b, hcl).await.unwrap();
    let note = std::fs::read_to_string(data.join("go-data/.imported")).unwrap();
    assert!(
        note.starts_with(
            "taken over from the Go doze-aws: 3 buckets, 5 objects, 4 queues, 5 messages"
        ),
        "{note}"
    );
    assert!(!note.contains("not carried"), "{note}");
    check_s3(&b);
    check_sqs(&b);

    // Once: a later converge carries nothing again.
    converge(&b, hcl).await.unwrap();
    let orders = aws(
        &b,
        &[
            "sqs",
            "get-queue-attributes",
            "--queue-url",
            &queue_url(&b, "orders"),
            "--attribute-names",
            "ApproximateNumberOfMessages",
        ],
    );
    assert_eq!(orders["Attributes"]["ApproximateNumberOfMessages"], "1");
    b.stop(Duration::from_secs(10)).await;
}

/// What the Go fixture held in S3, as the aws CLI now sees it.
fn check_s3(b: &Booted) {
    let versions = aws(
        b,
        &["s3api", "list-object-versions", "--bucket", "receipts"],
    );
    let one: Vec<&serde_json::Value> = versions["Versions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["Key"] == "r/one.txt")
        .collect();
    assert_eq!(one.len(), 2, "both versions: {versions}");
    let gone = &versions["DeleteMarkers"].as_array().unwrap()[0];
    assert_eq!(
        (gone["Key"].as_str(), gone["IsLatest"].as_bool()),
        (Some("r/gone.txt"), Some(true))
    );
    let out = std::env::temp_dir().join(format!("doze-aws-godata-{}", std::process::id()));
    let head = aws(
        b,
        &[
            "s3api",
            "get-object",
            "--bucket",
            "receipts",
            "--key",
            "r/one.txt",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "second version");
    assert_eq!(head["Metadata"]["shop"], "larder");
    let img = aws(
        b,
        &[
            "s3api",
            "get-object",
            "--bucket",
            "product-images",
            "--key",
            "products/BK 1.svg",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(
        (img["ContentType"].as_str(), img["CacheControl"].as_str()),
        (Some("image/svg+xml"), Some("max-age=60"))
    );
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "<svg/>");
    let tags = aws(
        b,
        &[
            "s3api",
            "get-object-tagging",
            "--bucket",
            "product-images",
            "--key",
            "products/BK 1.svg",
        ],
    );
    assert_eq!(tags["TagSet"][0]["Key"], "team");
    let bucket_tags = aws(
        b,
        &["s3api", "get-bucket-tagging", "--bucket", "undeclared"],
    );
    assert_eq!(bucket_tags["TagSet"][0]["Value"], "go");
    let _ = std::fs::remove_file(&out);
}

fn queue_url(b: &Booted, name: &str) -> String {
    aws(b, &["sqs", "get-queue-url", "--queue-name", name])["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// What the Go fixture held in SQS, as the aws CLI now sees it.
fn check_sqs(b: &Booted) {
    assert_eq!(queue_urls(b), ["events.fifo", "jobs", "jobs-dlq", "orders"]);
    let jobs = queue_url(b, "jobs");
    let attrs = aws(
        b,
        &[
            "sqs",
            "get-queue-attributes",
            "--queue-url",
            &jobs,
            "--attribute-names",
            "All",
        ],
    );
    assert_eq!(attrs["Attributes"]["VisibilityTimeout"], "45");
    assert!(
        attrs["Attributes"]["RedrivePolicy"]
            .as_str()
            .unwrap()
            .contains("jobs-dlq"),
        "{attrs}"
    );
    let got = aws(
        b,
        &[
            "sqs",
            "receive-message",
            "--queue-url",
            &jobs,
            "--max-number-of-messages",
            "10",
            "--message-attribute-names",
            "All",
        ],
    );
    let mut bodies: Vec<&str> = got["Messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["Body"].as_str().unwrap())
        .collect();
    bodies.sort_unstable();
    assert_eq!(
        bodies,
        ["job one", "job two"],
        "the message Go had in flight is visible again"
    );
    let first = got["Messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["Body"] == "job one")
        .unwrap();
    assert_eq!(first["MessageAttributes"]["kind"]["StringValue"], "pack");
    assert_eq!(first["MessageAttributes"]["blob"]["BinaryValue"], "AQID");
    check_fifo(b);
}

/// The FIFO queue gives Go's messages back in their order.
fn check_fifo(b: &Booted) {
    let fifo = aws(
        b,
        &[
            "sqs",
            "receive-message",
            "--queue-url",
            &queue_url(b, "events.fifo"),
            "--max-number-of-messages",
            "10",
        ],
    );
    let order: Vec<&str> = fifo["Messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["Body"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["e1", "e2"]);
}
