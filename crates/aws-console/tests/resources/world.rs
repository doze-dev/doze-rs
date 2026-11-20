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
    for (_, _, s) in &services {
        peers.register(Arc::clone(s));
    }
    services
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

pub(crate) fn field<'a>(detail: &'a Value, label: &str) -> &'a Value {
    detail["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["label"] == label)
        .unwrap_or_else(|| panic!("no field {label} in {detail}"))
}
