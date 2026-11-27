//! The observer: told once about every finished AWS API request, with what the gateway learnt of it, and about nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use aws_gateway::{Config, Finished, Gateway, Observer, WouldDeny};
use aws_model_sqs::{meta::META, tables::TABLES};
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Output, Reply, Service};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// SQS's model with nothing behind it: `ListQueues` answers empty, `PurgeQueue` is not built, anything else is a missing queue.
/// With a [`WouldDeny`], it plays soft IAM: `ListQueues` is let through as `sqs:ListQueues` refused.
#[derive(Debug, Default)]
struct FakeSqs(Option<Arc<WouldDeny>>);

impl Service for FakeSqs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        TABLES.op(op).unwrap().name != "PurgeQueue"
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            if let Some(flight) = &self.0
                && TABLES.op(call.op).unwrap().name == "ListQueues"
            {
                flight.let_through(
                    &call.caller.access_key_id,
                    &call.caller.region,
                    "sqs:ListQueues",
                );
            }
            // An identity policy allowed these two: the first policy told stands. A later check refused `DeleteQueue` all the same.
            if let Some(flight) = &self.0
                && matches!(
                    TABLES.op(call.op).unwrap().name,
                    "GetQueueUrl" | "DeleteQueue"
                )
            {
                let who = &call.caller;
                let action = format!("sqs:{}", TABLES.op(call.op).unwrap().name);
                flight.allowed_by(
                    &who.access_key_id,
                    &who.region,
                    &action,
                    "PowerUserAccess",
                    "aws-managed",
                );
                flight.allowed_by(&who.access_key_id, &who.region, &action, "second", "user");
            }
            if TABLES.op(call.op).unwrap().name == "DeleteQueue" {
                return Err(ApiError::new(
                    400,
                    "AccessDeniedException",
                    "a later check refused it",
                ));
            }
            match TABLES.op(call.op).unwrap().name {
                "ListQueues" => Ok(Output {
                    arena: aws_schema::arena::Arena::new(),
                    root: aws_schema::arena::NIL,
                }),
                _ => Err(ApiError::modeled(
                    "QueueDoesNotExist",
                    "The specified queue does not exist.",
                )),
            }
        })
    }
}

/// One call, as it was told.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Told {
    service: (&'static str, &'static str),
    action: &'static str,
    caller: String,
    region: String,
    status: u16,
    code: String,
    request_id: String,
    would_be_denied: bool,
    allowed_by: Option<(String, String)>,
    message: String,
    answer_bytes: Option<u32>,
    resource: String,
}

#[derive(Debug, Default)]
struct Tape(Mutex<Vec<Told>>);

impl Observer for Tape {
    fn finished(&self, call: &Finished<'_>) {
        self.0.lock().unwrap().push(Told {
            service: (call.signing_name, call.sdk_id),
            action: call.action,
            caller: call.access_key_id.to_owned(),
            region: call.region.to_owned(),
            status: call.status,
            code: call.code.to_owned(),
            request_id: call.request_id.to_owned(),
            would_be_denied: call.would_be_denied,
            allowed_by: call
                .allowed_by
                .map(|a| (a.policy.to_owned(), a.kind.to_owned())),
            message: call.message.to_owned(),
            answer_bytes: call.answer_bytes,
            resource: call.resource.to_owned(),
        });
    }
}

async fn start(observed: bool) -> (aws_gateway::Running, SocketAddr, Arc<Tape>) {
    start_with(observed, None).await
}

async fn start_with(
    observed: bool,
    soft: Option<Arc<WouldDeny>>,
) -> (aws_gateway::Running, SocketAddr, Arc<Tape>) {
    let tape = Arc::new(Tape::default());
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.health = Some(serde_json::Map::new());
    if observed {
        config.observer = Some(tape.clone());
    }
    config.would_deny.clone_from(&soft);
    let running = Gateway::new(config)
        .with(Arc::new(FakeSqs(soft)))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    (running, addr, tape)
}

/// One request; the status and the head (lower-cased).
async fn send(
    addr: SocketAddr,
    first: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut request = format!(
        "{first} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let head = text.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
    (head.split(' ').nth(1).unwrap().parse().unwrap(), head)
}

fn signed(target: &'static str, service: &str) -> Vec<(&'static str, String)> {
    vec![
        ("X-Amz-Target", target.to_owned()),
        ("X-Amz-Date", "20130524T000000Z".to_owned()),
        (
            "Authorization",
            format!(
                "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20130524/eu-west-1/{service}/aws4_request,SignedHeaders=host;x-amz-date,Signature=00"
            ),
        ),
    ]
}

async fn call(addr: SocketAddr, target: &'static str, service: &str, body: &str) -> (u16, String) {
    let headers = signed(target, service);
    let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    send(addr, "POST /", &headers, body).await
}

#[tokio::test]
async fn each_api_request_is_told_once_with_what_the_gateway_learnt() {
    let (running, addr, tape) = start(true).await;
    let (ok, head) = call(addr, "AmazonSQS.ListQueues", "sqs", "{}").await;
    let (missing, _) = call(addr, "AmazonSQS.GetQueueUrl", "sqs", r#"{"QueueName":"q"}"#).await;
    let (unbuilt, _) = call(addr, "AmazonSQS.PurgeQueue", "sqs", r#"{"QueueUrl":"u"}"#).await;
    let (invalid, _) = call(addr, "AmazonSQS.GetQueueUrl", "sqs", "{}").await;
    let (unknown_op, _) = call(addr, "AmazonSQS.NoSuchThing", "sqs", "{}").await;
    let (unknown_service, _) = call(addr, "Nope.Thing", "nope", "{}").await;
    let (unsigned, _) = send(addr, "POST /", &[], "{}").await;
    assert_eq!((ok, missing, unbuilt), (200, 400, 501));
    assert!(invalid >= 400 && unknown_op >= 400 && unknown_service >= 400 && unsigned >= 400);

    let told = tape.0.lock().unwrap().clone();
    assert_eq!(told.len(), 7, "{told:#?}");
    let sqs = ("sqs", "SQS");
    let first = &told[0];
    assert_eq!(
        (first.service, first.action, first.status),
        (sqs, "ListQueues", 200)
    );
    assert_eq!(
        (
            first.caller.as_str(),
            first.region.as_str(),
            first.code.as_str()
        ),
        ("AKIDEXAMPLE", "eu-west-1", "")
    );
    // The id told is the id the answer carried.
    assert!(
        head.contains(&format!("x-amzn-requestid: {}", first.request_id)),
        "{head}"
    );
    assert_eq!(
        (told[1].action, told[1].status, told[1].code.as_str()),
        (
            "GetQueueUrl",
            400,
            "AWS.SimpleQueueService.NonExistentQueue"
        )
    );
    assert!(told.iter().all(|t| !t.would_be_denied));
    // An operation this build does not serve is told by its name, with the refusal's code.
    assert_eq!(
        (told[2].action, told[2].status, told[2].code.as_str()),
        ("PurgeQueue", 501, "NotImplemented")
    );
    // A body the model refuses: the operation is known, and the code is the validation's.
    assert_eq!((told[3].service, told[3].action), (sqs, "GetQueueUrl"));
    assert!(!told[3].code.is_empty(), "{:?}", told[3]);
    // An operation the model does not have: the service is known, the action is not.
    assert_eq!((told[4].service, told[4].action), (sqs, ""));
    assert!(!told[4].code.is_empty(), "{:?}", told[4]);
    // A service this gateway does not have, and a request with no signature: neither names a service; both say why.
    assert_eq!(
        (told[5].service, told[5].code.as_str()),
        (("", ""), "UnrecognizedClientException")
    );
    // ...but a signed one keeps who signed it and for which region.
    assert_eq!(
        (told[5].caller.as_str(), told[5].region.as_str()),
        ("AKIDEXAMPLE", "eu-west-1")
    );
    assert_eq!(told[6].service, ("", ""));
    assert!(!told[6].code.is_empty(), "{:?}", told[6]);
    let ids: std::collections::BTreeSet<&str> =
        told.iter().map(|t| t.request_id.as_str()).collect();
    assert_eq!(ids.len(), 7, "every call has its own id");
    running.shutdown().await;
}

#[tokio::test]
async fn the_health_check_is_not_a_call_and_no_observer_is_no_trouble() {
    let (running, addr, tape) = start(true).await;
    let (status, _) = send(addr, "GET /_doze/health", &[], "").await;
    assert_eq!(status, 200);
    assert!(tape.0.lock().unwrap().is_empty());
    running.shutdown().await;

    let (running, addr, tape) = start(false).await;
    let (status, _) = call(addr, "AmazonSQS.ListQueues", "sqs", "{}").await;
    assert_eq!(status, 200);
    assert!(tape.0.lock().unwrap().is_empty());
    running.shutdown().await;
}

#[tokio::test]
async fn a_call_soft_iam_let_through_is_marked_and_no_other() {
    let flight = Arc::new(WouldDeny::default());
    let (running, addr, tape) = start_with(true, Some(flight.clone())).await;
    let (listed, _) = call(addr, "AmazonSQS.ListQueues", "sqs", "{}").await;
    // Another call by the same caller, which IAM allowed: not marked.
    let (missing, _) = call(addr, "AmazonSQS.GetQueueUrl", "sqs", r#"{"QueueName":"q"}"#).await;
    assert_eq!((listed, missing), (200, 400));
    let told = tape.0.lock().unwrap().clone();
    assert_eq!(
        told.iter()
            .map(|t| (t.action, t.would_be_denied))
            .collect::<Vec<_>>(),
        [("ListQueues", true), ("GetQueueUrl", false)]
    );
    // A refusal for a caller with nothing in flight marks nothing (and does not panic).
    flight.let_through("AKIDEXAMPLE", "eu-west-1", "sqs:ListQueues");
    running.shutdown().await;
}

#[tokio::test]
async fn an_error_answer_is_told_with_the_service_s_words_and_the_length_it_declared() {
    let (running, addr, tape) = start(true).await;
    let (status, _) = call(addr, "AmazonSQS.GetQueueUrl", "sqs", r#"{"QueueName":"q"}"#).await;
    assert_eq!(status, 400);
    let (ok, _) = call(addr, "AmazonSQS.ListQueues", "sqs", "{}").await;
    assert_eq!(ok, 200);
    let told = tape.0.lock().unwrap().clone();
    assert_eq!(told[0].message, "The specified queue does not exist.");
    assert!(told[0].answer_bytes.is_some_and(|n| n > 2), "{:?}", told[0]);
    // A success says nothing of the kind, and an empty JSON answer is two bytes.
    assert_eq!(told[1].message, "");
    assert_eq!(told[1].answer_bytes, Some(2));
    running.shutdown().await;
}

#[tokio::test]
async fn a_call_the_model_refuses_still_says_what_it_named() {
    let (running, addr, tape) = start(true).await;
    // A name the model's pattern does not allow is refused when the input is decoded, before any service sees it.
    let (refused, _) = call(
        addr,
        "AmazonSQS.CreateQueue",
        "sqs",
        r#"{"QueueName":"pick lists!"}"#,
    )
    .await;
    // A valid name the service does not know names it too, as before.
    let (missing, _) = call(addr, "AmazonSQS.GetQueueUrl", "sqs", r#"{"QueueName":"q"}"#).await;
    assert_eq!((refused, missing), (400, 400));
    let told = tape.0.lock().unwrap().clone();
    assert_eq!(told[0].action, "CreateQueue");
    assert_eq!(told[0].resource, "pick lists!", "{:?}", told[0]);
    assert_eq!(told[1].resource, "q");
    running.shutdown().await;
}

#[tokio::test]
async fn a_call_an_identity_policy_allowed_says_which_and_a_refused_one_says_nothing() {
    let flight = Arc::new(WouldDeny::default());
    let (running, addr, tape) = start_with(true, Some(flight.clone())).await;
    // Allowed by a policy, then answered an error of the service's own (the queue is missing): IAM's yes still stands. The first
    // policy told is the one kept.
    let (missing, _) = call(addr, "AmazonSQS.GetQueueUrl", "sqs", r#"{"QueueName":"q"}"#).await;
    // A call no policy was told for.
    let (listed, _) = call(addr, "AmazonSQS.ListQueues", "sqs", "{}").await;
    // A call a policy allowed and a later check refused: the answer is an access denial, and no policy is named.
    let (refused, _) = call(addr, "AmazonSQS.DeleteQueue", "sqs", r#"{"QueueUrl":"q"}"#).await;
    assert_eq!((missing, listed, refused), (400, 200, 400));
    let told = tape.0.lock().unwrap().clone();
    assert_eq!(
        told.iter()
            .map(|t| (t.action, t.allowed_by.clone()))
            .collect::<Vec<_>>(),
        [
            (
                "GetQueueUrl",
                Some(("PowerUserAccess".to_owned(), "aws-managed".to_owned()))
            ),
            ("ListQueues", None),
            ("DeleteQueue", None),
        ]
    );
    // Told of a caller with nothing in flight: marks nothing and does not panic.
    flight.allowed_by("AKIDEXAMPLE", "eu-west-1", "sqs:ListQueues", "p", "user");
    running.shutdown().await;
}
