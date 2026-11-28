//! doze-aws's weight after use: a small fixed workload over S3, SQS and DynamoDB (no Lambda, so no interpreter is needed), then
//! the wait for every service to be reaped as idle, then the resident set and the threads. What a server holds once its services
//! have gone back to sleep: threads that outlive a burst, a writer per database, memory an allocator kept. Reported, not yet limited
//! (docs/port/weight.md, "Workstream 2").
//!
//! The requests are plain HTTP/1.1 with a signature-shaped header: the server runs with `--auth trust` (its default), which reads
//! the key and the region from it and checks nothing else.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// How long a service sits idle before it is reaped, for this measurement.
pub(crate) const IDLE_SECS: u64 = 2;
/// The wait after the workload: the reap, the blocking threads' keep-alive (2 s), and a margin.
pub(crate) const SETTLE: Duration = Duration::from_secs(IDLE_SECS + 6);

/// What a server holds after use.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AfterUse {
    pub(crate) rss_kb: u64,
    pub(crate) threads: u64,
}

/// Runs the workload against the server on `port`.
pub(crate) fn workload(port: u16) -> Result<(), String> {
    let body = vec![b'w'; 64 << 10];
    s3(port, "PUT", "/weigh", b"")?;
    for i in 0..20 {
        s3(port, "PUT", &format!("/weigh/o{i}"), &body)?;
    }
    s3(port, "GET", "/weigh/o0", b"")?;
    let made = json(
        port,
        "sqs",
        "AmazonSQS.CreateQueue",
        r#"{"QueueName":"weigh"}"#,
    )?;
    let url = quoted_after(&made, "\"QueueUrl\":").ok_or("CreateQueue gave no QueueUrl")?;
    for i in 0..20 {
        let send = format!(r#"{{"QueueUrl":"{url}","MessageBody":"m{i}"}}"#);
        json(port, "sqs", "AmazonSQS.SendMessage", &send)?;
    }
    let receive = format!(r#"{{"QueueUrl":"{url}","MaxNumberOfMessages":10}}"#);
    json(port, "sqs", "AmazonSQS.ReceiveMessage", &receive)?;
    let table = r#"{"TableName":"weigh","AttributeDefinitions":[{"AttributeName":"k","AttributeType":"S"}],"KeySchema":[{"AttributeName":"k","KeyType":"HASH"}],"BillingMode":"PAY_PER_REQUEST"}"#;
    json(port, "dynamodb", "DynamoDB_20120810.CreateTable", table)?;
    for i in 0..20 {
        let item =
            format!(r#"{{"TableName":"weigh","Item":{{"k":{{"S":"k{i}"}},"v":{{"S":"v"}}}}}}"#);
        json(port, "dynamodb", "DynamoDB_20120810.PutItem", &item)?;
    }
    let get = r#"{"TableName":"weigh","Key":{"k":{"S":"k0"}}}"#;
    json(port, "dynamodb", "DynamoDB_20120810.GetItem", get)?;
    Ok(())
}

/// The resident set (KB) and thread count of `pid`.
pub(crate) fn after_use(pid: u32) -> Result<AfterUse, String> {
    Ok(AfterUse {
        rss_kb: super::weight::read_rss_kb(pid)?,
        threads: threads(pid)?,
    })
}

fn threads(pid: u32) -> Result<u64, String> {
    let status = Path::new("/proc").join(pid.to_string()).join("status");
    if let Ok(text) = std::fs::read_to_string(&status) {
        return text
            .lines()
            .find_map(|l| l.strip_prefix("Threads:"))
            .and_then(|n| n.trim().parse().ok())
            .ok_or_else(|| format!("{}: no Threads line", status.display()));
    }
    // macOS: `ps -M` prints a line per thread under a header.
    let out = Command::new("ps")
        .args(["-M", "-p", &pid.to_string()])
        .output()
        .map_err(|e| format!("ps: {e}"))?;
    let lines = String::from_utf8_lossy(&out.stdout).lines().count();
    u64::try_from(lines.saturating_sub(1)).map_err(|e| e.to_string())
}

fn s3(port: u16, method: &str, path: &str, body: &[u8]) -> Result<String, String> {
    request(port, method, path, "s3", &[], body)
}

fn json(port: u16, service: &str, target: &str, body: &str) -> Result<String, String> {
    let headers = [
        ("x-amz-target", target),
        ("content-type", "application/x-amz-json-1.0"),
    ];
    request(port, "POST", "/", service, &headers, body.as_bytes())
}

/// One request on a connection of its own; the answer's body, or why it was not a success.
fn request(
    port: u16,
    method: &str,
    path: &str,
    service: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<String, String> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n\
         Authorization: AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/{service}/aws4_request, SignedHeaders=host, Signature={}\r\n\
         x-amz-date: 20260101T000000Z\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\n",
        body.len(),
        "0".repeat(64)
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|e| e.to_string())?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&answer).into_owned();
    let status = text.split(' ').nth(1).unwrap_or("");
    if !status.starts_with('2') {
        return Err(format!(
            "{method} {path} ({service}): {}",
            text.lines().next().unwrap_or("no answer")
        ));
    }
    Ok(text
        .split_once("\r\n\r\n")
        .map_or_else(String::new, |(_, b)| b.to_owned()))
}

/// The JSON string after `key` in `text` (a crude read: the answer is ours and simple).
fn quoted_after(text: &str, key: &str) -> Option<String> {
    let rest = text.split_once(key)?.1.trim_start().strip_prefix('"')?;
    Some(rest.split_once('"')?.0.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_url_is_read_from_its_answer() {
        assert_eq!(
            quoted_after(r#"{"QueueUrl": "http://h/1/q"}"#, "\"QueueUrl\":").as_deref(),
            Some("http://h/1/q")
        );
        assert_eq!(quoted_after("{}", "\"QueueUrl\":"), None);
    }
}
