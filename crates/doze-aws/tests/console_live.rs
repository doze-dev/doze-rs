//! Live end to end through the real binary: a call names what it acted on, SNS delivering to SQS is recorded as a call between
//! services marked with the Publish that caused it, the map's flows count both, and a call is given back as commands.

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

fn start(dir: &Path) -> (Child, String, String) {
    let mut child = Command::new(BIN)
        .args(["--listen", "127.0.0.1:0", "--console", "127.0.0.1:0"])
        .args(["--services", "sqs,sns"])
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

fn signed(service: &str) -> String {
    format!(
        "X-Amz-Date: 20261114T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDLIVE/20261114/us-east-1/{service}/aws4_request, \
         SignedHeaders=host;x-amz-date, Signature=00"
    )
}

fn sqs(endpoint: &str, action: &str, body: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nX-Amz-Target: AmazonSQS.{action}\r\nContent-Type: application/x-amz-json-1.0\r\n{}",
        signed("sqs")
    );
    request(endpoint, &head, body)
}

fn sns(endpoint: &str, form: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\n{}",
        signed("sns")
    );
    request(endpoint, &head, &format!("{form}&Version=2010-03-31"))
}

fn api(console: &str, path: &str) -> Value {
    let answer = request(console, &format!("GET {path} HTTP/1.1\r\nX-Console: 1"), "");
    let body = answer.split_once("\r\n\r\n").unwrap().1;
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{path}: {e}: {answer}"))
}

fn stop(mut child: Child) {
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "doze-aws did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn find<'a>(calls: &'a [Value], action: &str) -> &'a Value {
    calls
        .iter()
        .find(|c| c["action"] == action)
        .unwrap_or_else(|| panic!("no {action} in {calls:?}"))
}

const TOPIC: &str = "arn:aws:sns:us-east-1:000000000000:placed";

fn ok(made: &str) {
    assert!(made.starts_with("HTTP/1.1 200"), "{made}");
}

/// A queue, a topic, the queue subscribed to it, and a message published.
fn fan_out(endpoint: &str) {
    ok(&sqs(endpoint, "CreateQueue", r#"{"QueueName":"orders"}"#));
    ok(&sns(endpoint, "Action=CreateTopic&Name=placed"));
    let queue = "arn:aws:sqs:us-east-1:000000000000:orders";
    ok(&sns(
        endpoint,
        &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint={queue}"),
    ));
    ok(&sns(
        endpoint,
        &format!("Action=Publish&TopicArn={TOPIC}&Message=hi"),
    ));
}

/// Every call names what it acted on; the delivery is SQS's SendMessage, made by SNS, caused by the Publish.
fn sees_the_delivery_via_sns(calls: &[Value]) {
    let created = find(calls, "CreateQueue");
    assert_eq!(
        (&created["member"], &created["resource"]),
        (&"QueueName".into(), &"orders".into())
    );
    let publish = find(calls, "Publish");
    assert_eq!(
        (&publish["member"], &publish["resource"]),
        (&"TopicArn".into(), &TOPIC.into())
    );
    assert!(publish["via"].is_null(), "{publish}");
    let sent = find(calls, "SendMessage");
    assert_eq!(
        (&sent["service"], &sent["via"]["by"]),
        (&"sqs".into(), &"sns".into()),
        "{sent}"
    );
    assert_eq!(sent["via"]["request_id"], publish["request_id"], "{sent}");
    assert_eq!(sent["member"], "QueueUrl");
    let url = sent["resource"].as_str().unwrap();
    assert!(url.ends_with("/000000000000/orders"), "{sent}");
    assert_eq!(
        (&sent["caller"], &sent["request_id"]),
        (&"AKIDLIVE".into(), &"".into())
    );
    // Recorded before the Publish: it was answered first.
    assert!(sent["seq"].as_u64() < publish["seq"].as_u64());
}

/// The map's flows: the key to each service, and SNS to SQS.
fn sees_the_flows(console: &str, newest: &Value) {
    let flows = api(console, "/api/v1/flows");
    let rows: Vec<(String, bool, String, u64)> = flows["flows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let text = |k: &str| f[k].as_str().unwrap().to_owned();
            (
                text("caller"),
                f["internal"].as_bool().unwrap(),
                text("service"),
                f["calls"].as_u64().unwrap(),
            )
        })
        .collect();
    for want in [
        ("AKIDLIVE", false, "sns", 3),
        ("AKIDLIVE", false, "sqs", 1),
        ("sns", true, "sqs", 1),
    ] {
        let want = (want.0.to_owned(), want.1, want.2.to_owned(), want.3);
        assert!(rows.contains(&want), "{want:?} not in {rows:?}");
    }
    assert_eq!(&flows["through_seq"], newest);
}

/// A call as commands: the CLI and curl with the one member the record has.
fn sees_the_commands(console: &str, endpoint: &str, calls: &[Value]) {
    let seq = |action: &str| find(calls, action)["seq"].clone();
    let copy = api(
        console,
        &format!("/api/v1/calls/{}/copy", seq("CreateQueue")),
    );
    let cli = format!(
        "aws --endpoint-url {endpoint} --region us-east-1 sqs create-queue --queue-name orders"
    );
    assert_eq!(copy["cli"], cli);
    let curl = copy["curl"].as_str().unwrap();
    assert!(
        curl.contains("X-Amz-Target: AmazonSQS.CreateQueue"),
        "{curl}"
    );
    assert!(curl.contains(r#"{"QueueName":"orders"}"#), "{curl}");
    assert_eq!(copy["filled"], serde_json::json!(["QueueName"]));
    let copy = api(console, &format!("/api/v1/calls/{}/copy", seq("Publish")));
    let curl = copy["curl"].as_str().unwrap();
    assert!(
        curl.contains("Action=Publish&Version=2010-03-31&TopicArn=arn%3Aaws%3Asns"),
        "{curl}"
    );
    assert_eq!(api(console, "/api/v1/calls/999/copy")["code"], "NOT_FOUND");
}

#[test]
fn sns_delivering_to_sqs_is_a_call_via_sns_caused_by_the_publish() {
    let dir = tempfile::tempdir().unwrap();
    let (child, endpoint, console) = start(dir.path());
    assert!(!console.is_empty(), "no console row");
    fan_out(&endpoint);
    let page = api(&console, "/api/v1/calls");
    let calls = page["calls"].as_array().unwrap();
    sees_the_delivery_via_sns(calls);
    sees_the_flows(&console, &calls.last().unwrap()["seq"]);
    sees_the_commands(&console, &endpoint, calls);
    stop(child);
}
