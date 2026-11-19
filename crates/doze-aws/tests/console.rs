//! `--console`, on the real binary: start doze-aws with the console on a port of its own, make an SQS call through the gateway,
//! and see it in the console's `/api/v1/calls` and on its event stream; the page is the placeholder, with the console's headers.
//! Without the flag there is no console, and an address beyond this machine is refused at start.

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

/// Starts the server with `args` and reads its ready block: the child and the block's rows by label.
fn start(dir: &Path, args: &[&str]) -> (Child, Vec<(String, String)>) {
    let mut child = Command::new(BIN)
        .args(["--listen", "127.0.0.1:0"])
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut rows = Vec::new();
    for line in lines.by_ref() {
        let line = line.unwrap();
        if let Some((label, value)) = line.trim().split_once(' ') {
            rows.push((label.to_owned(), value.trim().to_owned()));
        }
        if line.contains("when something looks wrong") {
            break;
        }
    }
    // The rest of standard output is drained so the server never blocks writing it.
    std::thread::spawn(move || lines.for_each(drop));
    (child, rows)
}

fn row<'a>(rows: &'a [(String, String)], label: &str) -> Option<&'a str> {
    rows.iter()
        .find(|(l, _)| l == label)
        .map(|(_, v)| v.as_str())
}

/// One request on a connection of its own; the whole answer as text.
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
    s.read_to_string(&mut out).unwrap();
    out
}

/// A JSON-protocol SQS call, as an SDK under `--auth trust` sends it (routed by its credential scope).
fn sqs(endpoint: &str, action: &str, body: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nX-Amz-Target: AmazonSQS.{action}\r\nContent-Type: application/x-amz-json-1.0\r\n\
         X-Amz-Date: 20261114T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDCONSOLE/20261114/us-west-2/sqs/aws4_request, \
         SignedHeaders=host;x-amz-date, Signature=00"
    );
    request(endpoint, &head, body)
}

/// A GET of the console's API, as its own page sends it; the body as JSON.
fn api(console: &str, path: &str) -> serde_json::Value {
    let answer = request(console, &format!("GET {path} HTTP/1.1\r\nX-Console: 1"), "");
    assert!(answer.starts_with("HTTP/1.1 200"), "{path}: {answer}");
    let body = answer.split_once("\r\n\r\n").unwrap().1;
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{path}: {e}: {body}"))
}

fn stop(mut child: Child) {
    // SIGTERM, as a supervisor stops it: the console and the gateway wind down and the process ends by itself.
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "doze-aws did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The page: the placeholder, with the console's headers.
fn sees_the_page(console: &str) {
    let page = request(console, "GET /live/calls HTTP/1.1", "");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    let lower = page.to_ascii_lowercase();
    assert!(
        lower.contains("content-security-policy: default-src 'self'"),
        "{page}"
    );
    assert!(lower.contains("x-frame-options: deny"), "{page}");
    assert!(
        page.contains("The doze-aws console is not built yet"),
        "{page}"
    );
}

/// What this doze-aws is.
fn sees_about(console: &str, endpoint: &str) {
    let about = api(console, "/api/v1/about");
    assert_eq!(about["endpoint"], endpoint);
    assert_eq!(about["console"], console);
    assert_eq!(
        (&about["auth"], &about["iam"]),
        (&"trust".into(), &"soft".into())
    );
    assert_eq!(about["account"], "000000000000");
    let served: Vec<&str> = about["services"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["served"] == true)
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(served, ["s3", "sqs"]);
    assert_eq!(about["services"].as_array().unwrap().len(), 18);
}

/// Every service of the build is counted, the ones left out of --services too (on the service, not its stand-in).
fn sees_every_service_counted(console: &str) {
    let services = api(console, "/api/v1/services");
    let rows = services["services"].as_array().unwrap();
    assert_eq!(rows.len(), 18);
    for r in rows {
        let (built, total) = (r["built"].as_u64().unwrap(), r["total"].as_u64().unwrap());
        assert!(built > 0 && built <= total, "{r}");
    }
    let dynamodb = rows.iter().find(|r| r["name"] == "dynamodb").unwrap();
    assert_eq!(
        dynamodb["apis"].as_array().unwrap().len(),
        2,
        "DynamoDB and its Streams: {dynamodb}"
    );
    assert_eq!(dynamodb["served"], false);
}

/// Opens a tail and reads its `ready` event: the connection and what was read.
fn open_tail(console: &str) -> (TcpStream, String) {
    let host = console.trim_start_matches("http://");
    let mut tail = TcpStream::connect(host).unwrap();
    tail.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        tail,
        "GET /api/v1/tail/live HTTP/1.1\r\nHost: {host}\r\nX-Console: 1\r\n\r\n"
    )
    .unwrap();
    let mut seen = String::new();
    let mut buf = [0_u8; 4096];
    while !seen.contains("\"reset\":") {
        let n = tail.read(&mut buf).unwrap();
        assert!(n > 0, "{seen}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    assert!(seen.contains("event: ready"), "{seen}");
    (tail, seen)
}

/// Reads the tail until it has said `needle`.
fn read_tail(tail: &mut TcpStream, seen: &mut String, needle: &str) {
    let mut buf = [0_u8; 4096];
    while !seen.contains(needle) {
        let n = tail.read(&mut buf).unwrap();
        assert!(n > 0, "the tail closed: {seen}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
}

/// The two calls, as `/api/v1/calls` has them; `made` is the answer the first caller was given.
fn sees_the_calls(console: &str, made: &str) {
    let page = api(console, "/api/v1/calls");
    let calls = page["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2, "{page}");
    let first = &calls[0];
    assert_eq!(
        (&first["service"], &first["api"], &first["action"]),
        (&"sqs".into(), &"SQS".into(), &"CreateQueue".into())
    );
    assert_eq!(
        (&first["status"], &first["outcome"]),
        (&200.into(), &"ok".into())
    );
    assert_eq!(
        (&first["caller"], &first["region"]),
        (&"AKIDCONSOLE".into(), &"us-west-2".into())
    );
    assert!(first["error_code"].is_null(), "{first}");
    assert!(first["duration_us"].as_u64().unwrap() > 0, "{first}");
    // The id in the record is the id the caller was answered with.
    let id = first["request_id"].as_str().unwrap();
    assert!(
        made.to_ascii_lowercase()
            .contains(&format!("x-amzn-requestid: {id}")),
        "{made}"
    );
    let second = &calls[1];
    assert_eq!(
        (&second["action"], &second["status"], &second["outcome"]),
        (&"GetQueueUrl".into(), &400.into(), &"error".into())
    );
    assert_eq!(second["error_code"], "QueueDoesNotExist");
    // The region the calls were signed for is one this doze-aws has seen.
    assert_eq!(
        api(console, "/api/v1/about")["regions"],
        serde_json::json!(["us-east-1", "us-west-2"])
    );
}

#[test]
fn a_call_through_the_gateway_shows_in_the_console() {
    let dir = tempfile::tempdir().unwrap();
    let (child, rows) = start(
        dir.path(),
        &["--console", "127.0.0.1:0", "--services", "sqs,s3"],
    );
    let endpoint = row(&rows, "endpoint").unwrap().to_owned();
    let console = row(&rows, "console")
        .unwrap_or_else(|| panic!("no console row in {rows:?}"))
        .to_owned();
    assert_ne!(endpoint, console, "the console has a port of its own");
    sees_the_page(&console);
    sees_about(&console, &endpoint);
    sees_every_service_counted(&console);

    // Nothing was called yet. Open a tail, then call SQS through the gateway.
    assert_eq!(
        api(&console, "/api/v1/calls")["calls"],
        serde_json::json!([])
    );
    let (mut tail, mut seen) = open_tail(&console);
    let made = sqs(&endpoint, "CreateQueue", r#"{"QueueName":"orders"}"#);
    assert!(made.starts_with("HTTP/1.1 200"), "{made}");
    let missing = sqs(&endpoint, "GetQueueUrl", r#"{"QueueName":"nope"}"#);
    assert!(missing.starts_with("HTTP/1.1 400"), "{missing}");
    sees_the_calls(&console, &made);

    // The tail was sent both.
    read_tail(&mut tail, &mut seen, "GetQueueUrl");
    assert!(
        seen.contains("event: calls") && seen.contains("CreateQueue"),
        "{seen}"
    );
    drop(tail);
    stop(child);
}

#[test]
fn without_the_flag_there_is_no_console_and_an_address_beyond_this_machine_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (child, rows) = start(dir.path(), &[]);
    assert!(row(&rows, "endpoint").is_some(), "{rows:?}");
    assert_eq!(row(&rows, "console"), None, "{rows:?}");
    stop(child);

    let out = Command::new(BIN)
        .args(["--listen", "127.0.0.1:0", "--console", "0.0.0.0:0"])
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("is not a loopback address"), "{said}");
    assert!(
        String::from_utf8_lossy(&out.stdout).is_empty(),
        "nothing was served"
    );
}
