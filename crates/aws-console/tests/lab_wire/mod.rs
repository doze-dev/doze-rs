//! What the Lab's wire tests share: a gateway with real SQS, SNS, S3, CloudWatch and Lambda behind it, the table and the recorder, and a raw HTTP client.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aws_console::Recorder;
use aws_console::lab::{Effect, LabTable, Matcher, Share, Spec};
use aws_console::ring::{Call, LabDid};
use aws_gateway::{Auth, Config, Gateway};
use doze_runtime::{Ctx, SystemClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// botocore's list of the codes it retries as a throttle (`ThrottledRetryableChecker`), and its transient statuses.
pub(crate) const THROTTLED: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];

pub(crate) struct Wire {
    pub(crate) running: aws_gateway::Running,
    pub(crate) addr: SocketAddr,
    pub(crate) table: Arc<LabTable>,
    pub(crate) recorder: Arc<Recorder>,
    pub(crate) dir: PathBuf,
}

impl Wire {
    pub(crate) async fn stop(self) {
        self.running.shutdown().await;
        drop(std::fs::remove_dir_all(&self.dir));
    }
}

pub(crate) async fn start(tag: &str) -> Wire {
    let dir = std::env::temp_dir().join(format!("doze-aws-lab-wire-{tag}-{}", std::process::id()));
    let ctx = |name: &str| {
        Arc::new(Ctx {
            dir: dir.join(name),
            idle: Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        })
    };
    let table = LabTable::new();
    let recorder = Arc::new(Recorder::new());
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.auth = Auth::Trust;
    config.observer = Some(recorder.clone());
    config.faults = Some(table.clone());
    let running = Gateway::new(config)
        .with(Arc::new(aws_sqs::Sqs::new(ctx("sqs"))))
        .with(Arc::new(aws_sns::Sns::new(ctx("sns"))))
        .with(Arc::new(aws_s3::S3::new(ctx("s3"))))
        .with(Arc::new(aws_cloudwatch::Cloudwatch::new(ctx("cloudwatch"))))
        .with(Arc::new(aws_lambda::Lambda::new(ctx("lambda"))))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    Wire {
        running,
        addr,
        table,
        recorder,
        dir,
    }
}

/// What the server sent back; no status at all when it closed the connection with no answer.
pub(crate) struct Answer {
    pub(crate) status: Option<u16>,
    pub(crate) head: String,
    pub(crate) body: Vec<u8>,
}

impl Answer {
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub(crate) fn scope(service: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential=AKIDLAB/20130524/us-east-1/{service}/aws4_request,SignedHeaders=host;x-amz-date,Signature=00"
    )
}

pub(crate) async fn send(
    addr: SocketAddr,
    service: &str,
    (method, path): (&str, &str),
    headers: &[(&str, &str)],
    body: &[u8],
) -> Answer {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nAuthorization: {}\r\nX-Amz-Date: 20130524T000000Z\r\nUser-Agent: Boto3/1.34.69\r\n",
        body.len(),
        scope(service)
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(head.as_bytes()).await.unwrap();
    // A body is sent when the server asks for it (`100 Continue`), as the SDKs do: a request answered before its body is read must
    // not have the body waiting in the socket when it is closed.
    let mut raw = Vec::new();
    if !body.is_empty()
        && headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("expect"))
    {
        let mut buf = [0u8; 4096];
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
        }
        if raw.starts_with(b"HTTP/1.1 100") {
            let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            raw.drain(..end);
            stream.write_all(body).await.unwrap();
        }
    } else {
        stream.write_all(body).await.unwrap();
    }
    // A connection closed by a drop may be reset instead: either way there is no answer.
    let mut rest = Vec::new();
    drop(stream.read_to_end(&mut rest).await);
    raw.extend_from_slice(&rest);
    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Answer {
            status: None,
            head: String::new(),
            body: raw,
        };
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    Answer {
        status: head.split(' ').nth(1).and_then(|s| s.parse().ok()),
        head,
        body: raw[split + 4..].to_vec(),
    }
}

pub(crate) fn rule(service: &str, operation: &str, effect: Effect) -> Spec {
    let (matcher, said) = Matcher::build(
        &[service.to_owned()],
        &[operation.to_owned()],
        &[],
        &[],
        &[],
    );
    assert!(said.is_empty());
    Spec {
        name: String::new(),
        matcher,
        effect,
        share: Share::Every,
        ttl: Duration::from_secs(600),
        max_hits: None,
    }
}

pub(crate) fn newest(w: &Wire) -> Call {
    w.recorder.newest(1).calls.pop().unwrap()
}

/// The newest call rule `id` touched.
pub(crate) fn tagged(w: &Wire, id: u32) -> Call {
    w.recorder.touched(Some(id), 1).0.pop().unwrap()
}

pub(crate) fn lab_of(c: &Call) -> Option<(u32, LabDid)> {
    c.extra
        .as_ref()
        .and_then(|e| e.lab.as_ref())
        .map(|l| (l.rule(), l.did()))
}
