//! What the tests of the record's summaries share: the binary started with its services, a raw signed request, and the console's calls read back.
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

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_doze-aws");

pub(crate) fn start(dir: &Path) -> (Child, String, String) {
    let mut child = Command::new(BIN)
        .args(["--listen", "127.0.0.1:0", "--console", "127.0.0.1:0"])
        .args([
            "--services",
            "sqs,sns,s3,dynamodb,kinesis,eventbridge,logs,cloudwatch,iam,sts,stepfunctions",
        ])
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
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

pub(crate) fn request(url: &str, head: &str, body: &str) -> String {
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
    s.read_to_string(&mut out).unwrap();
    out
}

pub(crate) fn signed(service: &str) -> String {
    format!(
        "X-Amz-Date: 20261114T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDSUM/20261114/us-east-1/{service}/aws4_request, \
         SignedHeaders=host;x-amz-date, Signature=00"
    )
}

pub(crate) fn query(endpoint: &str, service: &str, form: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\n{}",
        signed(service)
    );
    ok(&request(endpoint, &head, form))
}

pub(crate) fn ok(made: &str) -> String {
    assert!(made.starts_with("HTTP/1.1 200"), "{made}");
    made.to_owned()
}

pub(crate) fn api(console: &str, path: &str) -> Value {
    let answer = request(console, &format!("GET {path} HTTP/1.1\r\nX-Console: 1"), "");
    let body = answer.split_once("\r\n\r\n").unwrap().1;
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{path}: {e}: {answer}"))
}

pub(crate) fn stop(mut child: Child) {
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "doze-aws did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The summaries of the calls to `action`, oldest first (a call with none is `null`).
pub(crate) fn said(calls: &[Value], action: &str) -> Vec<Value> {
    calls
        .iter()
        .filter(|c| c["action"] == action && c["via"].is_null())
        .map(|c| c["summary"].clone())
        .collect()
}
