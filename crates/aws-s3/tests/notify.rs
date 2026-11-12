//! Bucket notifications to an SQS queue, over the peer bus: the test event when the configuration is put, then an event for each
//! change that fits the configuration's events and key filter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_gateway::{Config, Gateway};
use aws_peers::Peers;
use aws_wire::service::Caller;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const AUTH: &str = "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=00";

async fn s3(addr: SocketAddr, method: &str, path: &str, body: &str) -> u16 {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {AUTH}\r\nx-amz-date: 20260101T000000Z\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(body.as_bytes()).await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    raw.split(' ').nth(1).unwrap().parse().unwrap()
}

fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: "111122223333".into(),
        region: "us-east-1".into(),
        endpoint: "http://127.0.0.1".into(),
        service: None,
        session_token: None,
    }
}

/// The bodies of every message in the queue, parsed.
async fn drain(peers: &Peers, url: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for _ in 0..3 {
        let got = peers
            .call(
                "test",
                &caller(),
                "sqs",
                "ReceiveMessage",
                &json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "WaitTimeSeconds": 0}),
            )
            .await
            .unwrap();
        for m in got
            .get("Messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            out.push(serde_json::from_str(m["Body"].as_str().unwrap()).unwrap());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    out
}

#[tokio::test]
async fn events_reach_the_queue_that_the_configuration_names() {
    let dir = std::env::temp_dir().join(format!("aws-s3-notify-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let tasks = Tasks::new();
    let peers = Peers::new();
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let ctx = |name: &str| {
        Arc::new(Ctx {
            dir: dir.join(name),
            idle: Duration::from_secs(300),
            tasks: tasks.clone(),
            clock: Arc::new(SystemClock),
            ext: Some(Arc::clone(&ext)),
        })
    };
    let sqs: Arc<dyn aws_wire::service::Service> = Arc::new(aws_sqs::Sqs::new(ctx("sqs")));
    let store: Arc<dyn aws_wire::service::Service> = Arc::new(aws_s3::S3::new(ctx("s3")));
    peers.register(Arc::clone(&sqs));
    peers.register(Arc::clone(&store));
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap()))
        .with(store)
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();

    let made = peers
        .call(
            "test",
            &caller(),
            "sqs",
            "CreateQueue",
            &json!({"QueueName": "events"}),
        )
        .await
        .unwrap();
    let url = made["QueueUrl"].as_str().unwrap().to_owned();
    assert_eq!(s3(addr, "PUT", "/watched", "").await, 200);

    let arn = "arn:aws:sqs:us-east-1:111122223333:events";
    let missing = "arn:aws:sqs:us-east-1:111122223333:nowhere";
    let config = |arn: &str| {
        format!(
            "<NotificationConfiguration><QueueConfiguration><Queue>{arn}</Queue><Event>s3:ObjectCreated:*</Event><Event>s3:ObjectRemoved:Delete</Event><Filter><S3Key><FilterRule><Name>prefix</Name><Value>in/</Value></FilterRule></S3Key></Filter></QueueConfiguration></NotificationConfiguration>"
        )
    };
    assert_eq!(
        s3(addr, "PUT", "/watched?notification", &config(missing)).await,
        400,
        "a queue that does not exist is refused"
    );
    assert_eq!(
        s3(addr, "PUT", "/watched?notification", &config(arn)).await,
        200
    );
    let test = drain(&peers, &url).await;
    assert_eq!(test.len(), 1, "{test:?}");
    assert_eq!(test[0]["Event"], "s3:TestEvent");

    assert_eq!(s3(addr, "PUT", "/watched/in/a.txt", "hello").await, 200);
    assert_eq!(
        s3(addr, "PUT", "/watched/out/b.txt", "not watched").await,
        200
    );
    assert_eq!(s3(addr, "DELETE", "/watched/in/a.txt", "").await, 204);
    let events = drain(&peers, &url).await;
    let names: Vec<&str> = events
        .iter()
        .map(|e| e["Records"][0]["eventName"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["ObjectCreated:Put", "ObjectRemoved:Delete"],
        "{events:?}"
    );
    let first = &events[0]["Records"][0];
    assert_eq!(first["s3"]["bucket"]["name"], "watched");
    assert_eq!(first["s3"]["object"]["key"], "in/a.txt");
    assert_eq!(first["s3"]["object"]["size"], 5);

    running.shutdown().await;
    tasks.shutdown().await;
}
