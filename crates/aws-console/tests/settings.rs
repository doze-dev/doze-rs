//! Settings with the data engine doze-aws hands the console: the flags as given, the data by service, and a dry-run reset.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use serde_json::{Value, json};
use support::{HOST, keys, ts_keys};

/// A write as the console's own page sends it.
fn write(method: &str, path: &str, body: &Value) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", HOST)
        .header("x-console", "1")
        .header("origin", format!("http://{HOST}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

/// A data engine that answers as doze-aws's does, for what Settings makes of it.
#[derive(Debug)]
struct Engine;

impl aws_console::LocalData for Engine {
    fn report(&self) -> aws_wire::service::BoxFuture<'_, Result<Value, String>> {
        Box::pin(async {
            Ok(json!({"data_dir": "/d", "bytes": 300, "services": [
                {"name": "sqs", "title": "SQS", "bytes": 100},
                {"name": "s3", "title": "S3", "bytes": 200}
            ]}))
        })
    }

    fn reset(&self, dry_run: bool) -> aws_wire::service::BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move {
            Ok(
                json!({"services": [{"service": "sqs", "items": 2, "bytes": 100, "freed": if dry_run { 0 } else { 100 }}]}),
            )
        })
    }
}

#[tokio::test]
async fn settings_list_every_flag_and_reset_through_the_data_engine() {
    use std::sync::Arc;
    let mut facts = support::facts();
    facts.flags = vec![
        aws_console::Flag {
            name: "iam".into(),
            value: Some("soft".into()),
            source: "the default iam".into(),
            default: true,
        },
        aws_console::Flag {
            name: "secret-key".into(),
            value: None,
            source: "--secret-key".into(),
            default: false,
        },
    ];
    facts.data = Some(Arc::new(Engine));
    let state = aws_console::state::State::new(
        facts,
        Arc::new(aws_console::Recorder::new()),
        support::HOST.parse().unwrap(),
        Vec::new(),
        tokio_util::sync::CancellationToken::new(),
        &aws_console::Limits::default(),
    );
    let app = support::app(&state);
    let (status, view) = support::json(&app, support::get("/api/v1/settings")).await;
    assert_eq!(status, 200);
    assert_eq!(view["reset"], true);
    assert_eq!(view["data_bytes"], 300);
    assert_eq!(view["data"][0]["name"], "s3", "largest first");
    assert_eq!(
        view["flags"][1],
        json!({"name": "secret-key", "value": null, "source": "--secret-key", "default": false})
    );
    let (status, done) = support::json(
        &app,
        write("POST", "/api/v1/settings/reset", &json!({"dry_run": true})),
    )
    .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(keys(&done), ts_keys("ResetDone"));
    assert_eq!(
        (
            done["dry_run"].as_bool(),
            done["services"][0]["freed"].as_u64()
        ),
        (Some(true), Some(0))
    );
}
