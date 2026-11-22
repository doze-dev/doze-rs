//! What the module face's tests share: the binary started as doze starts it (`aws-plugin`), and the aws CLI against it.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;
use std::sync::OnceLock;

use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness};

pub(crate) fn plugin() -> PathBuf {
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

pub(crate) fn harness() -> Harness {
    Harness {
        program: plugin(),
        engine: "aws".into(),
        bin_dir: PathBuf::from("/unused"),
        full: "1".into(),
    }
}

pub(crate) fn endpoint(b: &Booted) -> String {
    match b.backend() {
        Some(Address::Tcp(a)) => format!("http://{a}"),
        other => panic!("a TCP backend, got {other:?}"),
    }
}

/// Whether the aws CLI is here; required when `DOZE_ACCEPTANCE` is set.
pub(crate) fn have_cli() -> bool {
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
pub(crate) fn aws(b: &Booted, args: &[&str]) -> serde_json::Value {
    aws_in(b, "us-east-1", args)
}

/// The aws CLI run for a client whose Region is `region`.
pub(crate) fn run_in(b: &Booted, region: &str, args: &[&str]) -> std::process::Output {
    std::process::Command::new("aws")
        .args(["--endpoint-url", &endpoint(b), "--output", "json"])
        .args(args)
        .env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("AWS_DEFAULT_REGION", region)
        .env("AWS_CONFIG_FILE", "/dev/null")
        .env("AWS_SHARED_CREDENTIALS_FILE", "/dev/null")
        .env("AWS_PAGER", "")
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_REGION")
        .env_remove("AWS_ENDPOINT_URL")
        .output()
        .unwrap()
}

/// What the aws CLI says on stderr when it fails for a client whose Region is `region`.
pub(crate) fn aws_fails(b: &Booted, region: &str, args: &[&str]) -> String {
    let out = run_in(b, region, args);
    assert!(!out.status.success(), "aws {args:?} succeeded");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub(crate) fn aws_in(b: &Booted, region: &str, args: &[&str]) -> serde_json::Value {
    let out = run_in(b, region, args);
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

pub(crate) fn queue_urls(b: &Booted) -> Vec<String> {
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

pub(crate) async fn converge(
    b: &Booted,
    hcl: &str,
) -> Result<(), doze_module_proto::msg::error::Error> {
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
