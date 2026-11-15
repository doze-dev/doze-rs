//! `/_doze/data`: the data engine over HTTP, for `doze-aws data` while a server runs and for the console.
//!
//! - `GET  /_doze/data[?service=s3]`            what is kept, service by service and resource by resource
//! - `POST /_doze/data/empty`   `{service, region?, name, prune}`  one resource's data, or the part the prune names
//! - `POST /_doze/data/reset`   `{service?, dry_run}`              everything (or one service), as if it never ran
//! - `POST /_doze/data/compact` `{}`                               the space deletes left, back to the disk
//!
//! These remove data without a signature, so they are guarded against the two ways a stranger could reach them. A web page the
//! person has open could post to `127.0.0.1`: a removal must be `application/json` (which a page cannot send across origins without
//! a preflight, and nothing here answers one) and an `Origin`, when sent, must be this server. And under `--auth verify` the server
//! may be reachable from elsewhere: every request must then carry the configured secret key in `X-Doze-Key`.

use std::sync::{Arc, OnceLock};

use aws_wire::service::{BoxFuture, Web, WebReply, WebRequest};
use serde_json::{Value, json};

use crate::data::{self, Data, Target};

const PATH: &str = "/_doze/data";

/// The hook. Made before the services are (the gateway is configured first); the engine is handed in once they exist.
pub(crate) struct DataHook {
    data: OnceLock<Arc<Data>>,
    key: Option<String>,
}

impl DataHook {
    /// A hook that, when `key` is set, answers only a request carrying it.
    pub(crate) fn new(key: Option<String>) -> Self {
        Self {
            data: OnceLock::new(),
            key,
        }
    }

    pub(crate) fn set(&self, data: Arc<Data>) {
        drop(self.data.set(data));
    }
}

fn header<'a>(req: &'a WebRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn reply(status: u16, body: &Value) -> WebReply {
    WebReply {
        status,
        headers: vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("cache-control".to_owned(), "no-store".to_owned()),
        ],
        body: serde_json::to_vec(body).unwrap_or_default().into(),
    }
}

fn error(status: u16, message: &str) -> WebReply {
    reply(status, &json!({"error": message}))
}

/// Why a request may not be answered, if it may not.
fn guard(req: &WebRequest, key: Option<&str>) -> Option<WebReply> {
    if let Some(key) = key
        && header(req, "x-doze-key") != Some(key)
    {
        return Some(error(
            403,
            "this doze-aws verifies signatures: send the configured secret key in X-Doze-Key",
        ));
    }
    if let Some(origin) = header(req, "origin") {
        let host = origin.split("://").nth(1).unwrap_or(origin);
        if host != req.host {
            return Some(error(
                403,
                "a page from another origin may not use /_doze/data",
            ));
        }
    }
    let json_body = header(req, "content-type").is_some_and(|t| t.starts_with("application/json"));
    (req.method == "POST" && !json_body).then(|| error(415, "send the request as application/json"))
}

impl Web for DataHook {
    fn owns(&self, _host: &str, path: &str) -> bool {
        path == PATH
            || path
                .strip_prefix(PATH)
                .is_some_and(|rest| rest.starts_with('/'))
    }

    fn serve(&self, req: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(async move {
            if let Some(refused) = guard(&req, self.key.as_deref()) {
                return refused;
            }
            let Some(data) = self.data.get() else {
                return error(503, "doze-aws is still starting");
            };
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            match (req.method.as_str(), req.path.as_str()) {
                ("GET", PATH) => report(data, service_param(&req.query).as_deref()).await,
                ("POST", "/_doze/data/empty") => empty(data, &body).await,
                ("POST", "/_doze/data/reset") => reset(data, &body).await,
                ("POST", "/_doze/data/compact") => match data.compact().await {
                    Ok(freed) => reply(200, &json!({"freed": freed})),
                    Err(r) => error(r.status, &r.message),
                },
                _ => error(
                    404,
                    "the data paths are GET /_doze/data and POST /_doze/data/{empty,reset,compact}",
                ),
            }
        })
    }
}

fn service_param(query: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("service="))
        .map(str::to_owned)
}

async fn report(data: &Data, only: Option<&str>) -> WebReply {
    match data.report(only).await {
        Ok(parts) => reply(200, &data::to_json(data.dir(), data.total(), &parts)),
        Err(r) => error(r.status, &r.message),
    }
}

async fn empty(data: &Data, body: &Value) -> WebReply {
    let text = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_owned);
    let (Some(service), Some(name)) = (text("service"), text("name")) else {
        return error(400, "say which: {\"service\": ..., \"name\": ...}");
    };
    let target = Target {
        service,
        region: text("region"),
        name,
        prune: data::prune_of(body.get("prune").unwrap_or(&Value::Null)),
    };
    match data.empty(&target).await {
        Ok(done) => reply(200, &done),
        Err(r) => error(r.status, &r.message),
    }
}

async fn reset(data: &Data, body: &Value) -> WebReply {
    let only = body.get("service").and_then(Value::as_str);
    let dry_run = body
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match data.reset(only, dry_run).await {
        Ok(done) => reply(200, &data::reset_json(&done)),
        Err(r) => error(r.status, &r.message),
    }
}
