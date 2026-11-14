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

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness};

fn plugin() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("aws-module-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = if cfg!(windows) {
            "aws-plugin.exe"
        } else {
            "aws-plugin"
        };
        let link = dir.join(exe);
        let _ = std::fs::remove_file(&link);
        let built = env!("CARGO_BIN_EXE_doze-aws");
        if std::fs::hard_link(built, &link).is_err() {
            std::fs::copy(built, &link).unwrap();
        }
        link
    })
    .clone()
}

fn harness() -> Harness {
    Harness {
        program: plugin(),
        engine: "aws".into(),
        bin_dir: PathBuf::from("/unused"),
        full: "1".into(),
    }
}

fn endpoint(b: &Booted) -> String {
    match b.backend() {
        Some(Address::Tcp(a)) => format!("http://{a}"),
        other => panic!("a TCP backend, got {other:?}"),
    }
}

/// Whether the aws CLI is here; required when `DOZE_ACCEPTANCE` is set.
fn have_cli() -> bool {
    let here = std::process::Command::new("aws")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(
        here || std::env::var_os("DOZE_ACCEPTANCE").is_none(),
        "DOZE_ACCEPTANCE is set: the aws CLI is needed"
    );
    here
}

/// The aws CLI against the instance, as a person would run it; its output as JSON.
fn aws(b: &Booted, args: &[&str]) -> serde_json::Value {
    let out = std::process::Command::new("aws")
        .args(["--endpoint-url", &endpoint(b), "--output", "json"])
        .args(args)
        .env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .env("AWS_CONFIG_FILE", "/dev/null")
        .env("AWS_SHARED_CREDENTIALS_FILE", "/dev/null")
        .env("AWS_PAGER", "")
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_ENDPOINT_URL")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "aws {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

fn queue_urls(b: &Booted) -> Vec<String> {
    let mut v: Vec<String> = aws(b, &["sqs", "list-queues"])["QueueUrls"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|u| u.as_str().unwrap().rsplit('/').next().unwrap().to_owned())
        .collect();
    v.sort();
    v
}

async fn converge(b: &Booted, hcl: &str) -> Result<(), doze_module_proto::msg::error::Error> {
    let mut inst = b.instance.clone();
    inst.spec = doze_module_test::hcl::parse_block(hcl, &b.module.manifest.schema)
        .unwrap()
        .spec;
    b.module
        .converge(&WithToolchain {
            instance: inst,
            toolchain: b.toolchain.clone(),
        })
        .await
}

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
