//! What the integration tests share: a console over one real service (SQS) and a recorder they feed by hand.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use aws_console::app::console;
use aws_console::state::{Shared, State};
use aws_console::{Facts, Limits, Recorder, ServiceFacts};
use aws_gateway::{Did, Finished};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// The `Host` the tests ask with: where the console of [`state`] says it listens.
pub(crate) const HOST: &str = "127.0.0.1:4567";

/// A doze-aws that serves SQS and has S3 in its catalogue with nothing behind it.
pub(crate) fn facts() -> Facts {
    let dir = std::env::temp_dir().join(format!("aws-console-test-{}", std::process::id()));
    let ctx = Arc::new(doze_runtime::Ctx {
        dir,
        idle: Duration::from_secs(300),
        tasks: doze_runtime::Tasks::new(),
        clock: Arc::new(doze_runtime::SystemClock),
        ext: None,
    });
    Facts {
        version: "9.9.9".to_owned(),
        account: "000000000000".to_owned(),
        region: "us-east-1".to_owned(),
        endpoint: "http://127.0.0.1:4566".to_owned(),
        auth: "trust".to_owned(),
        iam: "soft".to_owned(),
        data_dir: "/tmp/doze-aws-data".to_owned(),
        pid: 42,
        started: 1_800_000_000,
        services: vec![
            ServiceFacts {
                name: "s3",
                title: "S3",
                served: false,
                apis: Vec::new(),
            },
            ServiceFacts {
                name: "sqs",
                title: "SQS",
                served: true,
                apis: vec![Arc::new(aws_sqs::Sqs::new(ctx))],
            },
        ],
        peers: None,
        flags: Vec::new(),
        data: None,
        lab: aws_console::LabFacts::default(),
    }
}

/// The state of a console with an empty record of `capacity` calls.
pub(crate) fn state(capacity: usize, allow_hosts: &[&str]) -> Shared {
    State::new(
        facts(),
        Arc::new(Recorder::holding(capacity)),
        HOST.parse().unwrap(),
        allow_hosts.iter().map(|h| (*h).to_owned()).collect(),
        CancellationToken::new(),
        &Limits::default(),
    )
}

/// The console's router over `state`.
pub(crate) fn app(state: &Shared) -> Router {
    console(state.clone())
}

/// One finished SQS call.
pub(crate) fn sqs_call(action: &'static str, status: u16, code: &'static str) -> Finished<'static> {
    Finished {
        request_id: "0c6f3a52-7a8b-4c1d-9e2f-000000000000",
        signing_name: "sqs",
        sdk_id: "SQS",
        action,
        member: "",
        resource: "",
        access_key_id: "test",
        region: "us-east-1",
        status,
        code,
        took: Duration::from_micros(1_500),
        would_be_denied: false,
        user_agent: "Boto3/1.34.69 md/Botocore#1.34.69",
        message: "",
        function_error: "",
        answer_bytes: None,
        did: Did::NONE,
        lab: None,
    }
}

/// A GET as the console's own page sends it.
pub(crate) fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap()
}

/// The answer to `req` and its body as text.
pub(crate) async fn send(app: &Router, req: Request<Body>) -> (Response<()>, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (
        Response::from_parts(parts, ()),
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// The answer to `req`, as JSON.
pub(crate) async fn json(app: &Router, req: Request<Body>) -> (u16, serde_json::Value) {
    let (res, text) = send(app, req).await;
    let value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    (res.status().as_u16(), value)
}

/// The value of header `name` of `res`.
pub(crate) fn header_of<'a, B>(res: &'a Response<B>, name: &str) -> &'a str {
    res.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

/// The top-level keys of `export type Name = { .. }` in the generated file.
pub(crate) fn ts_keys(name: &str) -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../console/apps/aws/src/api/types")
        .join(format!("{name}.ts"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let mut plain = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("/*") {
        plain.push_str(&rest[..at]);
        rest = &rest[at + rest[at..].find("*/").unwrap() + 2..];
    }
    plain.push_str(rest);
    let body = &plain[plain.find("= {").unwrap() + 3..plain.rfind('}').unwrap()];
    let (mut depth, mut start, mut keys) = (0_i32, 0, BTreeSet::new());
    for (i, c) in body.char_indices() {
        match c {
            '{' | '[' | '<' | '(' => depth += 1,
            '}' | ']' | '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                if let Some((k, _)) = body[start..i].trim().split_once(':') {
                    keys.insert(k.trim().to_owned());
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    keys
}

pub(crate) fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object()
        .unwrap_or_else(|| panic!("not an object: {v}"))
        .keys()
        .cloned()
        .collect()
}
