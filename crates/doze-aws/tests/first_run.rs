//! The first minute, on the real binary: start it in an empty directory, read the ready block it prints, point a shell at it with
//! `env`, make a signed call, apply a template with an SQS queue and see `CREATE_COMPLETE`, ask `doctor`, and stop it. The time from
//! starting the process to its first answered call is printed (`cargo test -p doze-aws --test first_run -- --nocapture`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_doze-aws");

fn doze(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env_remove("AWS_ENDPOINT_URL")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (
        out.status.code().unwrap_or(-1),
        text(&out.stdout),
        text(&out.stderr),
    )
}

/// Starts the server (configured by the directory's doze-aws.toml) and reads its ready block; the child, the endpoint, the block.
fn start(dir: &Path) -> (Child, String, Vec<String>) {
    let mut child = Command::new(BIN)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut block = Vec::new();
    for line in lines.by_ref() {
        let line = line.unwrap();
        let done = line.contains("when something looks wrong");
        block.push(line);
        if done {
            break;
        }
    }
    // The rest of standard output is drained so the server never blocks writing it.
    std::thread::spawn(move || lines.for_each(drop));
    let endpoint = block
        .iter()
        .find_map(|l| l.trim().strip_prefix("endpoint"))
        .map(|e| e.trim().to_owned())
        .unwrap_or_else(|| panic!("no endpoint in {block:#?}"));
    (child, endpoint, block)
}

/// A JSON-protocol SQS call, as an SDK under `--auth trust` sends it (routed by its credential scope).
fn list_queues(endpoint: &str) -> String {
    let host = endpoint.trim_start_matches("http://");
    let mut s = TcpStream::connect(host).unwrap();
    let body = "{}";
    write!(
        s,
        "POST / HTTP/1.1\r\nHost: {host}\r\nX-Amz-Target: AmazonSQS.ListQueues\r\nContent-Type: application/x-amz-json-1.0\r\n\
         X-Amz-Date: 20261114T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=test/20261114/us-east-1/sqs/aws4_request, \
         SignedHeaders=host;x-amz-date, Signature=00\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg(child.id().to_string())
            .status()
            .unwrap();
    }
    #[cfg(not(unix))]
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn from_nothing_to_a_stack() {
    let dir = tempfile::tempdir().unwrap();
    // Every command reads the same file, so `env`, `apply` and `doctor` find the server by its data directory.
    std::fs::write(
        dir.path().join("doze-aws.toml"),
        "listen = \"127.0.0.1:0\"\ndata-dir = \"data\"\n",
    )
    .unwrap();
    let begun = Instant::now();
    let (mut child, endpoint, block) = start(dir.path());
    let answer = list_queues(&endpoint);
    let first_call = begun.elapsed();
    println!("first run: process start to first answered call: {first_call:?}");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let text = block.join("\n");
    assert!(text.contains("doze-aws is up"), "{text}");
    assert!(
        text.contains("17 services in us-east-1, account 000000000000"),
        "{text}"
    );
    assert!(text.contains("doze-aws doctor"), "{text}");

    let (code, env, _) = doze(dir.path(), &["env", "--shell", "sh"]);
    assert_eq!(code, 0);
    assert!(
        env.contains(&format!("export AWS_ENDPOINT_URL={endpoint}\n")),
        "{env}"
    );
    assert!(env.contains("export AWS_ACCESS_KEY_ID=test\n"), "{env}");

    std::fs::write(
        dir.path().join("template.yaml"),
        "Resources:\n  Orders:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: orders\n",
    )
    .unwrap();
    let (code, out, err) = doze(dir.path(), &["apply", "--stack", "shop"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("CREATE_COMPLETE") && out.contains("AWS::SQS::Queue"),
        "{out}"
    );
    assert!(
        list_queues(&endpoint).contains("/orders"),
        "the queue is there"
    );
    let (code, out, _) = doze(dir.path(), &["apply", "--stack", "shop"]);
    assert_eq!(code, 0);
    assert!(out.contains("nothing to change"), "{out}");
    let (_, out, _) = doze(dir.path(), &["export", "--stack", "shop"]);
    assert!(out.contains("AWS::SQS::Queue"), "{out}");

    let (_, out, _) = doze(dir.path(), &["doctor"]);
    assert!(out.contains("ok  running"), "{out}");

    let (code, _, err) = doze(dir.path(), &[]);
    assert_eq!(code, 1);
    assert!(
        err.contains("already running") && err.contains(&endpoint),
        "{err}"
    );

    stop(&mut child);
    assert!(
        !dir.path().join("data/instance.json").exists(),
        "a clean stop removes instance.json"
    );
}

#[test]
fn a_typo_is_an_error_and_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let begun = Instant::now();
    let (code, _, err) = doze(dir.path(), &["aply"]);
    assert_eq!(code, 2);
    assert!(err.contains("apply"), "{err}");
    assert!(!dir.path().join("doze-aws-data").exists());
    assert!(begun.elapsed() < Duration::from_secs(5));
}
