//! The module face of doze-aws, driven as doze drives it: the binary started as `aws-plugin` by the host, over module
//! protocol 2, and the core's boot sequence from `doze-module-test`; then the real aws CLI against the instance.
//!
//! - the published example boots, converges, and what it declares is there as the CLI sees it;
//! - better than Go, each on the running instance: a changed argument is updated in place (Go only created what was
//!   missing); a removed block keeps its resource; an unchanged block changes nothing; a converge the stack refuses
//!   says what failed.
//!
//! Needs the `aws` CLI (GitHub's runners have it); without it the CLI checks are skipped unless `DOZE_ACCEPTANCE` is set.
#![cfg(feature = "module")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod module_support;

use std::time::Duration;

use module_support::{aws, converge, harness, have_cli, queue_urls};

#[tokio::test(flavor = "current_thread")]
async fn the_published_example_boots_and_the_aws_cli_sees_what_it_declares() {
    let b = harness().boot_example().await.unwrap();
    assert!(b.fresh);
    if have_cli() {
        assert_eq!(
            queue_urls(&b),
            ["emails", "emails-dlq", "orders.fifo"],
            "the auto dead-letter queue is made beside its queue"
        );
        let v = aws(
            &b,
            &["s3api", "get-bucket-versioning", "--bucket", "uploads"],
        );
        assert_eq!(v["Status"], "Enabled");
        let p = aws(
            &b,
            &["ssm", "get-parameter", "--name", "/app/feature/banner"],
        );
        assert_eq!(p["Parameter"]["Value"], "hello");
        let s = aws(
            &b,
            &[
                "secretsmanager",
                "get-secret-value",
                "--secret-id",
                "db_password",
            ],
        );
        assert_eq!(s["SecretString"], "s3cr3t");
        let t = aws(
            &b,
            &["dynamodb", "describe-table", "--table-name", "sessions"],
        );
        assert_eq!(t["Table"]["KeySchema"][0]["AttributeName"], "session_id");
    }
    b.stop(Duration::from_secs(20)).await;
}

const SMALL: &str = r#"aws "local" {
  queue "jobs" {
    visibility = 30
  }
  queue "spare" {}
  parameter "/app/mode" { value = "one" }
}
"#;

#[tokio::test(flavor = "current_thread")]
async fn converge_updates_in_place_keeps_what_left_and_says_what_failed() {
    let b = harness().boot(SMALL).await.unwrap();
    if !have_cli() {
        b.stop(Duration::from_secs(20)).await;
        return;
    }
    let jobs = || {
        let url = aws(&b, &["sqs", "get-queue-url", "--queue-name", "jobs"])["QueueUrl"]
            .as_str()
            .unwrap()
            .to_owned();
        aws(
            &b,
            &[
                "sqs",
                "get-queue-attributes",
                "--queue-url",
                &url,
                "--attribute-names",
                "VisibilityTimeout",
            ],
        )["Attributes"]["VisibilityTimeout"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(jobs(), "30");
    // Changed in place (Go's converge created only what was missing), and `spare` taken out of the block.
    let changed = SMALL
        .replace("visibility = 30", "visibility = 45")
        .replace("  queue \"spare\" {}\n", "")
        .replace("\"one\"", "\"two\"");
    converge(&b, &changed).await.unwrap();
    assert_eq!(jobs(), "45");
    let p = aws(&b, &["ssm", "get-parameter", "--name", "/app/mode"]);
    assert_eq!(p["Parameter"]["Value"], "two");
    assert_eq!(
        queue_urls(&b),
        ["jobs", "spare"],
        "a removed block keeps its queue"
    );
    // The same again changes nothing and succeeds.
    converge(&b, &changed).await.unwrap();
    // A converge the stack cannot make fails, names what failed, and leaves the rest as it was.
    let bad = changed.replace(
        "parameter \"/app/mode\"",
        "state_machine \"broken\" {\n    definition = \"{}\"\n  }\n  parameter \"/app/mode\"",
    );
    let e = converge(&b, &bad).await.unwrap_err();
    assert!(e.message.contains("StateMachine"), "{e:?}");
    assert_eq!(jobs(), "45");
    b.stop(Duration::from_secs(20)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_mistake_is_refused_before_boot_at_its_line() {
    let e = harness()
        .boot("aws \"local\" {\n  queue \"a\" {\n    dlq = \"nope\"\n  }\n}\n")
        .await
        .unwrap_err();
    assert!(e.contains("no queue \"nope\" is declared"), "{e}");
}
