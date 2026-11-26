//! The harness of the Resources tests: real services on one bus, a console over them, and requests as its pages send them.

use std::sync::Arc;
use std::time::Duration;

use aws_console::app::console;
use aws_console::state::{Shared, State};
use aws_console::{Facts, Limits, Recorder, ServiceFacts};
use aws_peers::{Mode, Peers};
use aws_wire::service::Service;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::support::{HOST, get, json as answer, keys, ts_keys};

/// A doze-aws's worth of services on one bus, watched by a Live record, with IAM deciding in `mode`.
pub(crate) struct World {
    pub(crate) app: Router,
    pub(crate) state: Shared,
    dir: std::path::PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.dir));
    }
}

type Named = Vec<(&'static str, &'static str, Arc<dyn Service>)>;

/// The services, each with its own directory under `dir`, on `peers` with IAM deciding in `mode`.
fn services(dir: &std::path::Path, peers: &Arc<Peers>, mode: Mode) -> Named {
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let ctx = |n: &str| {
        Arc::new(doze_runtime::Ctx {
            dir: dir.join(n),
            idle: Duration::from_secs(300),
            tasks: doze_runtime::Tasks::new(),
            clock: Arc::new(doze_runtime::SystemClock),
            ext: Some(Arc::clone(&ext)),
        })
    };
    let (iam, sts) = aws_iam::services(ctx("iam"));
    let iam = Arc::new(iam);
    peers.set_mode(mode);
    peers.set_authorizer(iam.clone());
    let services: Named = vec![
        ("iam", "IAM", iam),
        ("sts", "STS", Arc::new(sts)),
        ("sqs", "SQS", Arc::new(aws_sqs::Sqs::new(ctx("sqs")))),
        ("sns", "SNS", Arc::new(aws_sns::Sns::new(ctx("sns")))),
        ("s3", "S3", Arc::new(aws_s3::S3::new(ctx("s3")))),
        (
            "dynamodb",
            "DynamoDB",
            Arc::new(aws_dynamodb::Dynamodb::new(ctx("dynamodb"))),
        ),
        (
            "lambda",
            "Lambda",
            Arc::new(aws_lambda::Lambda::new(ctx("lambda"))),
        ),
        (
            "secretsmanager",
            "Secrets Manager",
            Arc::new(aws_secrets::Secrets::new(ctx("secretsmanager"))),
        ),
        (
            "ssm",
            "Parameter Store",
            Arc::new(aws_ssm::Ssm::new(ctx("ssm"))),
        ),
        ("kms", "KMS", Arc::new(aws_kms::Kms::new(ctx("kms")))),
    ];
    let mut services = services;
    services.extend(more(&ctx));
    for (_, _, s) in &services {
        peers.register(Arc::clone(s));
    }
    // API Gateway's two APIs sign alike: the bus names the second `apigatewayv2`, as doze-aws does.
    let (rest, http) = aws_apigateway::services(ctx("apigateway"));
    let (rest, http): (Arc<dyn Service>, Arc<dyn Service>) = (Arc::new(rest), Arc::new(http));
    peers.register_as("apigateway", Arc::clone(&rest));
    peers.register_as("apigatewayv2", http);
    services.push(("apigateway", "API Gateway", rest));
    services
}

/// The services of Resources' second part.
fn more(ctx: &dyn Fn(&str) -> Arc<doze_runtime::Ctx>) -> Named {
    vec![
        (
            "kinesis",
            "Kinesis",
            Arc::new(aws_kinesis::Kinesis::new(ctx("kinesis"))),
        ),
        (
            "logs",
            "CloudWatch Logs",
            Arc::new(aws_logs::Logs::new(ctx("logs"))),
        ),
        (
            "cloudwatch",
            "CloudWatch",
            Arc::new(aws_cloudwatch::Cloudwatch::new(ctx("cloudwatch"))),
        ),
        (
            "eventbridge",
            "EventBridge",
            Arc::new(aws_eventbridge::Eventbridge::new(ctx("eventbridge"))),
        ),
        (
            "scheduler",
            "Scheduler",
            Arc::new(aws_scheduler::Scheduler::new(ctx("scheduler"))),
        ),
        (
            "stepfunctions",
            "Step Functions",
            Arc::new(aws_sfn::Sfn::new(ctx("states"))),
        ),
        (
            "cloudformation",
            "CloudFormation",
            Arc::new(aws_cloudformation::Cloudformation::new(ctx(
                "cloudformation",
            ))),
        ),
    ]
}

pub(crate) fn world(name: &str, mode: Mode, served: &[&'static str]) -> World {
    let dir = std::env::temp_dir().join(format!("aws-console-res-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let peers = Peers::new();
    let services = services(&dir, &peers, mode);
    let recorder = Arc::new(Recorder::new());
    peers.watch_calls(recorder.clone());
    let facts = Facts {
        version: "9.9.9".to_owned(),
        account: "000000000000".to_owned(),
        region: "us-east-1".to_owned(),
        endpoint: "http://127.0.0.1:4566".to_owned(),
        auth: "trust".to_owned(),
        iam: "enforce".to_owned(),
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
        peers: Some(peers),
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
    World {
        app: console(state.clone()),
        state,
        dir,
    }
}

/// A write as the console's own page sends it.
pub(crate) fn write(method: &str, path: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

// Each test binary uses some of these.
#[allow(dead_code)]
impl World {
    pub(crate) async fn get(&self, path: &str) -> (u16, Value) {
        answer(&self.app, get(path)).await
    }

    pub(crate) async fn send(&self, method: &str, path: &str, body: &Value) -> (u16, Value) {
        answer(&self.app, write(method, path, body)).await
    }

    /// Creates one, then finds it in the list (shown as `label`, unless that is empty) and opens it; the name it was made as.
    pub(crate) async fn round_trip(&self, base: &str, form: &Value, label: &str) -> String {
        let (status, made) = self.send("POST", base, form).await;
        assert_eq!(status, 201, "{base}: {made}");
        assert_eq!(keys(&made), ts_keys("Created"));
        let name = made["name"].as_str().unwrap().to_owned();
        let (status, list) = self.get(base).await;
        assert_eq!(status, 200, "{list}");
        assert_eq!(keys(&list), ts_keys("ResourceList"));
        let row = list["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name.as_str())
            .unwrap_or_else(|| panic!("{name} is not in {list}"));
        assert_eq!(keys(row), ts_keys("ResourceRow"));
        if !label.is_empty() {
            assert_eq!(row["label"], label);
        }
        let one = format!("{base}/{}", encode(&name));
        let (status, detail) = self.get(&one).await;
        assert_eq!(status, 200, "{detail}");
        assert_eq!(keys(&detail), ts_keys("ResourceDetail"));
        assert_eq!(detail["name"], name.as_str());
        name
    }

    pub(crate) async fn delete(&self, base: &str, name: &str) {
        let one = format!("{base}/{}", encode(name));
        let (status, done) = self.send("DELETE", &one, &json!({})).await;
        assert_eq!(status, 200, "{done}");
        let (_, list) = self.get(base).await;
        assert!(
            !list["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["name"] == name),
            "{name} is still listed: {list}"
        );
    }

    /// A call as an app's would be: to `target` on the bus, with an access key of its own (`app`), the account root's.
    pub(crate) async fn app(&self, target: &str, op: &str, body: &Value) -> Value {
        self.app_with("app", target, op, body).await
    }

    /// A call as the holder of the access key `key` would make it.
    pub(crate) async fn app_with(&self, key: &str, target: &str, op: &str, body: &Value) -> Value {
        let caller = aws_wire::service::Caller {
            access_key_id: key.to_owned(),
            account_id: "000000000000".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint: "http://127.0.0.1:4566".to_owned(),
            service: None,
            session_token: None,
        };
        let peers = self.state.facts.peers.clone().unwrap();
        let answer = if peers.speaks_rest(target) {
            peers
                .call_rest_as(&caller, target, op, body, None)
                .await
                .map(|r| r.json)
        } else {
            peers.call_as(&caller, target, op, body).await
        };
        answer.unwrap_or_else(|e| panic!("{target} {op}: {e:?}"))
    }

    /// Every call in the Live record, as `caller service action`.
    pub(crate) fn live(&self) -> Vec<String> {
        self.state
            .recorder
            .newest(usize::MAX)
            .calls
            .iter()
            .map(|c| format!("{} {} {}", c.caller, c.signing_name, c.action))
            .collect()
    }
}

/// As `encodeURIComponent` writes a name into an address (what the tests' names need).
pub(crate) fn encode(name: &str) -> String {
    name.replace('%', "%25")
        .replace('/', "%2F")
        .replace(':', "%3A")
}

#[allow(dead_code)]
pub(crate) fn field<'a>(detail: &'a Value, label: &str) -> &'a Value {
    detail["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["label"] == label)
        .unwrap_or_else(|| panic!("no field {label} in {detail}"))
}

/// The titles of a detail's related lists, and each one's rows' first cells.
#[allow(dead_code)]
pub(crate) fn sub<'a>(detail: &'a Value, title: &str) -> &'a Value {
    detail["subs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["title"] == title)
        .unwrap_or_else(|| panic!("no list {title} in {detail}"))
}

#[allow(dead_code)]
pub(crate) fn firsts(list: &Value) -> Vec<String> {
    assert_eq!(list["error"], Value::Null, "{list}");
    list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["cells"][0].as_str().unwrap_or_default().to_owned())
        .collect()
}
