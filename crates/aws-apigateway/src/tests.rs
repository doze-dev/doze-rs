//! API Gateway end to end inside the crate: the control plane through the services as the gateway calls them, and the execute-api
//! plane through [`Web::serve`] (and, for WebSocket, through a real gateway on a socket).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used
)]

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::{Harness, TempDir};
use aws_wire::ApiError;
use aws_wire::service::{Service, Web, WebReply, WebRequest};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::{ApiGateway, ApiGatewayV2};

mod logs;
mod openapi;
mod plane;
mod rest;
mod tls;
mod v2;
mod websocket;

pub(crate) use plane::backend as plane_backend;

/// Both APIs on a temporary directory.
pub(crate) struct World {
    pub(crate) v1: ApiGateway,
    pub(crate) v2: ApiGatewayV2,
    h1: Harness<ApiGateway>,
    h2: Harness<ApiGatewayV2>,
    pub(crate) tasks: Tasks,
    _dir: TempDir,
}

impl World {
    pub(crate) fn new() -> Self {
        Self::on(None)
    }

    /// With CloudWatch Logs on a peer bus, for the logs a stage writes.
    pub(crate) fn with_logs() -> (Self, Arc<aws_peers::Peers>) {
        let peers = aws_peers::Peers::new();
        peers.set_mode(aws_peers::Mode::Off);
        let w = Self::on(Some(peers.clone()));
        let logs = aws_logs::Logs::new(Arc::new(Ctx {
            dir: w._dir.path().join("logs"),
            idle: Duration::from_secs(300),
            tasks: w.tasks.clone(),
            clock: Arc::new(SystemClock),
            ext: Some(peers.clone()),
        }));
        peers.register(Arc::new(logs));
        (w, peers)
    }

    fn on(peers: Option<Arc<aws_peers::Peers>>) -> Self {
        let dir = TempDir::new("apigateway");
        let tasks = Tasks::new();
        let ctx = Arc::new(Ctx {
            dir: dir.path().join("apigateway"),
            idle: Duration::from_secs(300),
            tasks: tasks.clone(),
            clock: Arc::new(SystemClock),
            ext: peers.map(|p| p as Arc<dyn std::any::Any + Send + Sync>),
        });
        let (v1, v2) = crate::services(ctx);
        Self {
            h1: Harness::new(v1.clone()),
            h2: Harness::new(v2.clone()),
            v1,
            v2,
            tasks,
            _dir: dir,
        }
    }

    /// A REST API (v1) operation.
    pub(crate) async fn rest(&self, op: &str, req: Value) -> Result<Value, ApiError> {
        let op = self.h1.op(op);
        let call = self.h1.call(op, &req)?;
        let out = self.v1.call(call).await?;
        Ok(self.h1.json(op, &out))
    }

    /// An HTTP API (v2) operation.
    pub(crate) async fn http(&self, op: &str, req: Value) -> Result<Value, ApiError> {
        let op = self.h2.op(op);
        let call = self.h2.call(op, &req)?;
        let out = self.v2.call(call).await?;
        Ok(self.h2.json(op, &out))
    }

    /// An execute-api request.
    pub(crate) async fn web(
        &self,
        method: &str,
        path: &str,
        body: &str,
        headers: &[(&str, &str)],
    ) -> WebReply {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        assert!(self.v1.owns("localhost", path), "{path} is not the plane's");
        let request = WebRequest {
            method: method.into(),
            host: "localhost:4566".into(),
            path: path.into(),
            query: query.into(),
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            body: bytes::Bytes::copy_from_slice(body.as_bytes()),
            endpoint: "http://localhost:4566".into(),
        };
        self.v1.serve(request).await
    }

    /// A REST API with one resource at `/<part>` and a `GET` on it, as `(api, resource)`.
    pub(crate) async fn rest_api_with(&self, part: &str) -> (String, String) {
        let api = self
            .rest("CreateRestApi", json!({"name": "t"}))
            .await
            .unwrap();
        let id = api["id"].as_str().unwrap().to_owned();
        let root = api["rootResourceId"].as_str().unwrap().to_owned();
        let res = self
            .rest(
                "CreateResource",
                json!({"restApiId": id, "parentId": root, "pathPart": part}),
            )
            .await
            .unwrap();
        let rid = res["id"].as_str().unwrap().to_owned();
        self.rest(
            "PutMethod",
            json!({"restApiId": id, "resourceId": rid, "httpMethod": "GET", "authorizationType": "NONE"}),
        )
        .await
        .unwrap();
        (id, rid)
    }

    /// A MOCK integration on `GET` with one `200` response whose template is `body`.
    pub(crate) async fn mock(&self, api: &str, rid: &str, request: &str, body: &str) {
        let at = json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET"});
        let with = |extra: Value| {
            let mut v = at.clone();
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            v
        };
        let mut put =
            with(json!({"type": "MOCK", "requestTemplates": {"application/json": request}}));
        put.as_object_mut()
            .unwrap()
            .insert("requestHttpMethod".into(), json!("GET"));
        put.as_object_mut().unwrap().remove("httpMethod");
        self.rest("PutIntegration", put).await.unwrap();
        self.rest("PutMethodResponse", with(json!({"statusCode": "200"})))
            .await
            .unwrap();
        self.rest(
            "PutIntegrationResponse",
            with(json!({"statusCode": "200", "responseTemplates": {"application/json": body}})),
        )
        .await
        .unwrap();
    }
}

/// A reply's body as JSON.
pub(crate) fn body(r: &WebReply) -> Value {
    serde_json::from_slice(&r.body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&r.body).into_owned()))
}

/// A reply's header.
pub(crate) fn header<'a>(r: &'a WebReply, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
