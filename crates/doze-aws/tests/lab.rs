//! The Lab through the real binary: a rule made in the console bends the calls a client sends to the port, Stop all lets them through,
//! and `--lab=false` leaves the console with no rules to make.

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

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_doze-aws");

fn start(dir: &Path, extra: &[&str]) -> (Child, String, String) {
    let mut child = Command::new(BIN)
        .args(["--listen", "127.0.0.1:0", "--console", "127.0.0.1:0"])
        .args(["--services", "sqs"])
        .args(extra)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || BufReader::new(stderr).lines().for_each(drop));
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let (mut endpoint, mut console) = (String::new(), String::new());
    for line in lines.by_ref() {
        let line = line.unwrap();
        if let Some((label, value)) = line.trim().split_once(' ') {
            match label {
                "endpoint" => value.trim().clone_into(&mut endpoint),
                "console" => value.trim().clone_into(&mut console),
                _ => {}
            }
        }
        if line.contains("when something looks wrong") {
            break;
        }
    }
    std::thread::spawn(move || lines.for_each(drop));
    (child, endpoint, console)
}

fn request(url: &str, head: &str, body: &str) -> String {
    let host = url.trim_start_matches("http://");
    let mut s = TcpStream::connect(host).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "{head}\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn sqs(endpoint: &str, action: &str, body: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nX-Amz-Target: AmazonSQS.{action}\r\nContent-Type: application/x-amz-json-1.0\r\nX-Amz-Date: 20261114T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDLAB/20261114/us-east-1/sqs/aws4_request, SignedHeaders=host;x-amz-date, Signature=00"
    );
    request(endpoint, &head, body)
}

/// The console's API as its page calls it: the proof header, and for a write the same origin.
fn api(console: &str, method: &str, path: &str, body: &Value) -> Value {
    let head = format!(
        "{method} {path} HTTP/1.1\r\nX-Console: 1\r\nOrigin: {console}\r\nContent-Type: application/json"
    );
    let answer = request(console, &head, &body.to_string());
    let text = answer.split_once("\r\n\r\n").unwrap().1;
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{path}: {e}: {answer}"))
}

fn stop(mut child: Child) {
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "doze-aws did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("doze-aws-lab-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_rule_made_in_the_console_bends_the_calls_to_the_port_until_stop_all() {
    let d = dir("on");
    let (child, endpoint, console) = start(&d, &[]);
    let rules = api(&console, "GET", "/api/v1/lab/rules", &json!(null));
    assert_eq!(rules["enabled"], true, "{rules}");
    let made = api(
        &console,
        "POST",
        "/api/v1/lab/rules",
        &json!({
            "matcher": { "services": ["sqs"], "operations": ["CreateQueue"], "regions": [], "callers": [], "resources": [] },
            "effect": { "kind": "fail", "error": "throttle" },
            "share": { "kind": "every" },
            "ends_in_secs": 600,
        }),
    );
    assert_eq!(made["outcome"], "made", "{made}");
    let id = made["rule"]["id"].as_u64().unwrap();

    let refused = sqs(&endpoint, "CreateQueue", r#"{"QueueName":"orders"}"#);
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(
        refused
            .to_ascii_lowercase()
            .contains("x-amzn-query-error: requestthrottled;sender"),
        "{refused}"
    );
    // Other operations go through; the refused call wrote nothing.
    let listed = sqs(&endpoint, "ListQueues", "{}");
    assert!(listed.starts_with("HTTP/1.1 200"), "{listed}");
    assert!(!listed.contains("orders"), "{listed}");

    // Live holds the call with the rule's tag, and the rule counted it.
    let calls = api(
        &console,
        "GET",
        &format!("/api/v1/calls?lab={id}"),
        &json!(null),
    );
    assert_eq!(calls["calls"][0]["lab"]["id"], id, "{calls}");
    assert_eq!(
        calls["calls"][0]["lab"]["effect"],
        json!({ "kind": "failed" })
    );
    assert_eq!(calls["calls"][0]["error_code"], "RequestThrottled");
    let one = api(
        &console,
        "GET",
        &format!("/api/v1/lab/rules/{id}"),
        &json!(null),
    );
    assert_eq!(one["rule"]["hits"], 1);

    let stopped = api(&console, "POST", "/api/v1/lab/stop-all", &json!({}));
    assert_eq!(stopped["stopped"], 1);
    let made = sqs(&endpoint, "CreateQueue", r#"{"QueueName":"orders"}"#);
    assert!(made.starts_with("HTTP/1.1 200"), "{made}");
    assert!(made.contains("orders"));
    stop(child);
    drop(std::fs::remove_dir_all(&d));
}

#[test]
fn with_lab_false_the_console_has_no_rules_to_make_and_the_port_is_never_bent() {
    let d = dir("off");
    let (child, endpoint, console) = start(&d, &["--lab=false"]);
    let rules = api(&console, "GET", "/api/v1/lab/rules", &json!(null));
    assert_eq!(rules["enabled"], false, "{rules}");
    assert!(rules["off_why"].as_str().unwrap().contains("--lab=false"));
    let made = api(
        &console,
        "POST",
        "/api/v1/lab/rules",
        &json!({ "effect": { "kind": "drop" }, "ends_in_secs": 60 }),
    );
    assert_eq!(made["code"], "LAB_OFF", "{made}");
    let ok = sqs(&endpoint, "ListQueues", "{}");
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    stop(child);
    drop(std::fs::remove_dir_all(&d));
}

#[test]
fn a_bare_lab_flag_is_on() {
    let d = dir("flag");
    let (child, _, console) = start(&d, &["--lab"]);
    assert_eq!(
        api(&console, "GET", "/api/v1/lab/rules", &json!(null))["enabled"],
        true
    );
    stop(child);
    drop(std::fs::remove_dir_all(&d));
}
