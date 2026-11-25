//! What the hook's tests share: a fake SQS that counts the calls it is asked, a scripted table, an observer's tape, and a raw client.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_gateway::{Ask, Config, Done, Failure, Faults, Finished, Gateway, Observer, Tag, Verdict};
use aws_model_sqs::{meta::META, tables::TABLES};
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::service::{BoxFuture, Call, Output, Reply, Service};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// SQS's model with nothing behind it that counts the calls it is asked.
#[derive(Debug, Default)]
pub(crate) struct CountingSqs(pub(crate) AtomicUsize);

impl Service for CountingSqs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn call(&self, _call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Output {
                arena: aws_schema::arena::Arena::new(),
                root: aws_schema::arena::NIL,
            })
        })
    }
}

/// A [`Faults`] that answers every request it is asked about with one verdict, and notes what it was shown.
#[derive(Debug)]
pub(crate) struct Script {
    pub(crate) armed: AtomicBool,
    pub(crate) verdict: Mutex<Verdict>,
    pub(crate) judged: AtomicUsize,
    pub(crate) seen: Mutex<Vec<String>>,
}

impl Script {
    pub(crate) fn new(verdict: Verdict) -> Arc<Self> {
        Arc::new(Self {
            armed: AtomicBool::new(true),
            verdict: Mutex::new(verdict),
            judged: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl Faults for Script {
    fn armed(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    fn judge(&self, ask: &Ask<'_>) -> Verdict {
        self.judged.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(format!(
            "{}|{}|{}|{}|{}|{}",
            ask.signing_name,
            ask.sdk_id,
            ask.operation,
            ask.resource,
            ask.access_key_id,
            ask.region
        ));
        self.verdict.lock().unwrap().clone()
    }
}

pub(crate) fn tag() -> Tag {
    Tag {
        rule: 7,
        name: Arc::from("throttle orders"),
    }
}

pub(crate) fn throttle() -> Verdict {
    Verdict::Fail(Box::new(Failure {
        shape: Some("RequestThrottled"),
        status: 403,
        code: "RequestThrottled".into(),
        message: "Rate exceeded.".to_owned(),
        tag: tag(),
    }))
}

/// What the observer was told of a request, with the rule that touched it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) action: String,
    pub(crate) status: u16,
    pub(crate) code: String,
    pub(crate) took: Duration,
    pub(crate) lab: Option<(u32, String, Done)>,
}

#[derive(Debug, Default)]
pub(crate) struct Tape(pub(crate) Mutex<Vec<Told>>);

impl Observer for Tape {
    fn finished(&self, call: &Finished<'_>) {
        self.0.lock().unwrap().push(Told {
            action: call.action.to_owned(),
            status: call.status,
            code: call.code.to_owned(),
            took: call.took,
            lab: call.lab.map(|l| (l.rule, l.name.to_owned(), l.done)),
        });
    }
}

pub(crate) struct Harness {
    pub(crate) running: aws_gateway::Running,
    pub(crate) addr: SocketAddr,
    pub(crate) service: Arc<CountingSqs>,
    pub(crate) tape: Arc<Tape>,
}

pub(crate) async fn start(faults: Option<Arc<Script>>, observed: bool) -> Harness {
    let service = Arc::new(CountingSqs::default());
    let tape = Arc::new(Tape::default());
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    if observed {
        config.observer = Some(tape.clone());
    }
    config.faults = faults.map(|f| f as Arc<dyn Faults>);
    let running = Gateway::new(config)
        .with(service.clone())
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    Harness {
        running,
        addr,
        service,
        tape,
    }
}

/// One `SendMessage`; the raw bytes the server sent back (empty when it closed the connection with no answer).
pub(crate) async fn send_raw(addr: SocketAddr, body: &str) -> Vec<u8> {
    let request = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/x-amz-json-1.0\r\nX-Amz-Target: AmazonSQS.SendMessage\r\nX-Amz-Date: 20130524T000000Z\r\nUser-Agent: Boto3/1.34.69\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDLAB/20130524/eu-west-1/sqs/aws4_request,SignedHeaders=host;x-amz-date,Signature=00\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    // A closed connection may be a reset: that is no answer either.
    drop(stream.read_to_end(&mut raw).await);
    raw
}

pub(crate) const SEND: &str =
    r#"{"QueueUrl":"http://localhost:4566/000000000000/orders","MessageBody":"hi"}"#;

pub(crate) fn text(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).to_ascii_lowercase()
}
