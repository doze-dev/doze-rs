//! The harness of the forced events' tests: the services a rehearsal acts on, really, on one bus with one task set (so the register of
//! lazy services sees them all), a console over them, and requests as the Lab's page sends them. DynamoDB reads a clock the test owns.

#![allow(dead_code, clippy::too_many_lines)]

use std::sync::Arc;
use std::time::Duration;

use aws_console::app::console;
use aws_console::state::{Shared, State};
use aws_console::{Facts, Limits, Recorder, ServiceFacts};
use aws_peers::{Mode, Peers};
use aws_wire::service::{Caller, Service};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use doze_runtime::{ManualClock, Tasks};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::support::{HOST, get, json as answer};

pub(crate) struct Lab {
    pub(crate) app: Router,
    pub(crate) state: Shared,
    pub(crate) peers: Arc<Peers>,
    pub(crate) tasks: Tasks,
    /// What DynamoDB reads as the time.
    pub(crate) clock: Arc<ManualClock>,
    dir: std::path::PathBuf,
}

impl Drop for Lab {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.dir));
    }
}

/// A doze-aws's worth of the services a forced event is about, `served` of them served (all, when it is empty).
pub(crate) fn lab(name: &str, served: &[&'static str]) -> Lab {
    lab_in(name, served, Mode::Off)
}

/// The same with IAM deciding in `mode` (the console calls as the account's root, which it lets do what the person running doze-aws can).
pub(crate) fn lab_in(name: &str, served: &[&'static str], mode: Mode) -> Lab {
    let dir = std::env::temp_dir().join(format!("aws-console-once-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let peers = Peers::new();
    peers.set_mode(mode);
    let tasks = Tasks::new();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let ctx = |n: &str, clock: Arc<dyn doze_runtime::Clock>| {
        Arc::new(doze_runtime::Ctx {
            dir: dir.join(n),
            idle: Duration::from_secs(300),
            tasks: tasks.clone(),
            clock,
            ext: Some(Arc::clone(&ext)),
        })
    };
    let system = || -> Arc<dyn doze_runtime::Clock> { Arc::new(doze_runtime::SystemClock) };
    let services: Vec<(&'static str, &'static str, Arc<dyn Service>)> = vec![
        (
            "sqs",
            "SQS",
            Arc::new(aws_sqs::Sqs::new(ctx("sqs", system()))),
        ),
        (
            "dynamodb",
            "DynamoDB",
            Arc::new(aws_dynamodb::Dynamodb::new(ctx("dynamodb", clock.clone()))),
        ),
        (
            "lambda",
            "Lambda",
            Arc::new(aws_lambda::Lambda::new(ctx("lambda", system()))),
        ),
        (
            "scheduler",
            "EventBridge Scheduler",
            Arc::new(aws_scheduler::Scheduler::new(ctx("scheduler", system()))),
        ),
        (
            "kms",
            "KMS",
            Arc::new(aws_kms::Kms::new(ctx("kms", system()))),
        ),
    ];
    for (_, _, s) in &services {
        peers.register(Arc::clone(s));
    }
    if mode != Mode::Off {
        let (iam, sts) = aws_iam::services(ctx("iam", system()));
        let iam = Arc::new(iam);
        peers.register(iam.clone());
        peers.register(Arc::new(sts));
        peers.set_authorizer(iam);
    }
    peers.set_rests(tasks.rests());
    let recorder = Arc::new(Recorder::new());
    peers.watch_calls(recorder.clone());
    let facts = Facts {
        version: "9.9.9".to_owned(),
        account: "000000000000".to_owned(),
        region: "us-east-1".to_owned(),
        endpoint: "http://127.0.0.1:4566".to_owned(),
        auth: "trust".to_owned(),
        iam: "off".to_owned(),
        data_dir: dir.display().to_string(),
        pid: 1,
        started: 1,
        services: services
            .iter()
            .map(|(n, title, s)| ServiceFacts {
                name: n,
                title,
                served: served.is_empty() || served.contains(n),
                apis: vec![Arc::clone(s)],
            })
            .collect(),
        peers: Some(peers.clone()),
        flags: Vec::new(),
        data: None,
        lab: aws_console::LabFacts::default(),
    };
    let state = State::new(
        facts,
        recorder,
        HOST.parse().unwrap(),
        Vec::new(),
        CancellationToken::new(),
        &Limits::default(),
    );
    Lab {
        app: console(state.clone()),
        state,
        peers,
        tasks,
        clock,
        dir,
    }
}

/// A write as the Lab's page sends it.
pub(crate) fn write(path: &str, body: &Value, region: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(r) = region {
        req = req.header("x-console-region", r);
    }
    req.body(Body::from(body.to_string())).unwrap()
}

pub(crate) const ONCE: &str = "/api/v1/lab/once";

impl Lab {
    pub(crate) async fn get(&self, path: &str) -> (u16, Value) {
        answer(&self.app, get(path)).await
    }

    pub(crate) async fn force(&self, kind: &str, body: &Value) -> (u16, Value) {
        answer(&self.app, write(&format!("{ONCE}/{kind}"), body, None)).await
    }

    /// A call on the bus as an app's would be, in `region`.
    pub(crate) async fn app(&self, region: &str, target: &str, op: &str, body: &Value) -> Value {
        let caller = Caller {
            access_key_id: "app".to_owned(),
            account_id: "000000000000".to_owned(),
            region: region.to_owned(),
            endpoint: "http://127.0.0.1:4566".to_owned(),
            service: None,
            session_token: None,
        };
        let done = if self.peers.speaks_rest(target) {
            self.peers
                .call_rest_as(&caller, target, op, body, None)
                .await
                .map(|r| r.json)
        } else {
            self.peers.call_as(&caller, target, op, body).await
        };
        done.unwrap_or_else(|e| panic!("{target} {op}: {} {}", e.code, e.message))
    }

    /// The calls the Live record holds, as `service action` (and `via` when a service made it).
    pub(crate) fn live(&self) -> Vec<String> {
        self.state
            .recorder
            .newest(usize::MAX)
            .calls
            .iter()
            .rev()
            .map(|c| {
                let via = c
                    .via
                    .as_ref()
                    .filter(|v| !v.by.is_empty() || !v.name.is_empty())
                    .map_or(String::new(), |v| format!(" via {} {}", v.by, v.name));
                format!("{} {}{via}", c.signing_name, c.action)
            })
            .collect()
    }

    /// Whether the service `name` runs in `region`.
    pub(crate) fn runs(&self, name: &str, region: &str) -> Option<bool> {
        self.tasks.rests().is_live(name, region)
    }
}

pub(crate) fn sent(sentence: &Value) -> &str {
    sentence.as_str().unwrap_or_default()
}

/// Whether `zip` is on this machine (a test that needs to make a function says so and passes without it).
pub(crate) fn have_zip() -> bool {
    std::process::Command::new("sh")
        .args(["-c", "command -v zip"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The base64 of a zip holding one file, made with the `zip` program.
pub(crate) fn function_zip() -> String {
    let dir = std::env::temp_dir().join(format!("aws-console-once-zip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bootstrap");
    std::fs::write(&file, "#!/bin/sh\n").unwrap();
    let zip = dir.join("f.zip");
    drop(std::fs::remove_file(&zip));
    let ok = std::process::Command::new("zip")
        .args(["-qj", zip.to_str().unwrap(), file.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(ok.success());
    let text = aws_core::base64::encode(&std::fs::read(&zip).unwrap());
    drop(std::fs::remove_dir_all(&dir));
    text
}

pub(crate) fn queue(name: &str) -> Value {
    json!({"QueueName": name})
}
