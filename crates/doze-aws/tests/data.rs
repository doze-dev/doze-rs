//! `doze-aws data` on the real binary: with a server running (over `/_doze/data`), the hook's guards against a page in a browser,
//! and with the server stopped (the command opens the services itself). The same queue is seen, emptied and reset both ways.

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

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_doze-aws");

fn doze(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (
        out.status.code().unwrap_or(-1),
        text(&out.stdout),
        text(&out.stderr),
    )
}

fn start(dir: &Path) -> (Child, String) {
    let mut child = Command::new(BIN)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut endpoint = None;
    for line in lines.by_ref() {
        let line = line.unwrap();
        if let Some(e) = line.trim().strip_prefix("endpoint") {
            endpoint = Some(e.trim().to_owned());
        }
        if line.contains("when something looks wrong") {
            break;
        }
    }
    std::thread::spawn(move || lines.for_each(drop));
    (child, endpoint.expect("an endpoint in the ready block"))
}

fn stop(child: &mut Child) {
    #[cfg(unix)]
    Command::new("kill")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    #[cfg(not(unix))]
    child.kill().unwrap();
    child.wait().unwrap();
}

/// A raw HTTP exchange; the status line and the body.
fn http(endpoint: &str, head: &str, body: &str) -> (String, String) {
    let host = endpoint.trim_start_matches("http://");
    let mut s = TcpStream::connect(host).unwrap();
    write!(
        s,
        "{head}\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.lines().next().unwrap_or_default().to_owned();
    let body = out.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
    (status, body)
}

fn sqs(endpoint: &str, op: &str, body: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nX-Amz-Target: AmazonSQS.{op}\r\nContent-Type: application/x-amz-json-1.0\r\n\
         X-Amz-Date: 20261115T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=test/20261115/us-east-1/sqs/aws4_request, \
         SignedHeaders=host;x-amz-date, Signature=00"
    );
    let (status, body) = http(endpoint, &head, body);
    assert!(status.contains("200"), "{op}: {status} {body}");
    body
}

fn report(dir: &Path) -> Value {
    let (code, out, err) = doze(dir, &["data", "--json"]);
    assert_eq!(code, 0, "{err}");
    serde_json::from_str(&out).unwrap()
}

fn queue<'a>(report: &'a Value, name: &str) -> &'a Value {
    let sqs = report["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "sqs")
        .expect("sqs in the report");
    sqs["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .expect("the queue")
}

#[test]
fn a_queue_is_seen_emptied_and_reset_with_and_without_a_server() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("doze-aws.toml"),
        "listen = \"127.0.0.1:0\"\ndata-dir = \"data\"\n",
    )
    .unwrap();
    let (mut child, endpoint) = start(dir.path());
    let url = sqs(&endpoint, "CreateQueue", r#"{"QueueName":"jobs"}"#);
    let url: Value = serde_json::from_str(&url).unwrap();
    let url = url["QueueUrl"].as_str().unwrap();
    let body = "x".repeat(4000);
    for _ in 0..50 {
        sqs(
            &endpoint,
            "SendMessage",
            &format!(r#"{{"QueueUrl":"{url}","MessageBody":"{body}"}}"#),
        );
    }

    // Seen through the running server.
    let seen = report(dir.path());
    assert_eq!(queue(&seen, "jobs")["items"], 50);
    let (code, text, _) = doze(dir.path(), &["data"]);
    assert_eq!(code, 0);
    assert!(text.contains("SQS") && text.contains("1 queue"), "{text}");

    // A page in a browser cannot remove anything: not as a form, not from another origin.
    let (status, _) = http(
        &endpoint,
        "POST /_doze/data/compact HTTP/1.1\r\nContent-Type: text/plain",
        "{}",
    );
    assert!(status.contains("415"), "{status}");
    let (status, _) = http(
        &endpoint,
        "POST /_doze/data/compact HTTP/1.1\r\nContent-Type: application/json\r\nOrigin: https://evil.example",
        "{}",
    );
    assert!(status.contains("403"), "{status}");

    // No terminal and no --yes: nothing is removed.
    let (code, _, err) = doze(dir.path(), &["data", "empty", "sqs", "jobs"]);
    assert_eq!(code, 1);
    assert!(err.contains("--yes"), "{err}");
    assert_eq!(queue(&report(dir.path()), "jobs")["items"], 50);

    let (code, out, err) = doze(dir.path(), &["data", "empty", "sqs", "jobs", "--yes"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Removed 50 messages"), "{out}");
    assert_eq!(queue(&report(dir.path()), "jobs")["items"], 0);
    // The queue is still there and takes messages.
    sqs(
        &endpoint,
        "SendMessage",
        &format!(r#"{{"QueueUrl":"{url}","MessageBody":"again"}}"#),
    );
    stop(&mut child);

    // With the server stopped, the command opens the services itself.
    assert_eq!(queue(&report(dir.path()), "jobs")["items"], 1);
    let (code, out, err) = doze(dir.path(), &["data", "reset", "--yes"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Done"), "{out}");
    let after = report(dir.path());
    let sqs_part = after["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "sqs")
        .unwrap();
    assert!(
        sqs_part["resources"].as_array().unwrap().is_empty(),
        "{after}"
    );

    // A server starts on the data directory afterwards: the command let go of its lock.
    let (mut child, endpoint) = start(dir.path());
    sqs(&endpoint, "CreateQueue", r#"{"QueueName":"jobs"}"#);
    stop(&mut child);
}
