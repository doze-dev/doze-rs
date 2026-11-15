//! Where `doze-aws data` finds the engine: the running doze-aws over `/_doze/data`, or the services opened in this process (holding
//! the data directory's lock, so a server cannot start meanwhile). Either way the answers are the same JSON documents.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::Config;
use crate::data::{self, Data, Target};

/// Where the engine is.
pub(crate) enum Via {
    Server(crate::client::Client, Option<String>),
    Local(Local),
}

/// The engine in this process, its runtime, and the lock that keeps a server from starting under it.
pub(crate) struct Local {
    runtime: tokio::runtime::Runtime,
    data: Arc<Data>,
    tasks: doze_runtime::Tasks,
    _lock: doze_runtime::DataDirLock,
}

impl Via {
    pub(crate) fn open(cfg: &Config) -> Result<Self, String> {
        if let Some(found) = crate::locate::find(cfg) {
            let key = (found.auth == "verify").then(|| found.keys().1);
            return Ok(Self::Server(found.client()?, key));
        }
        let dir = cfg.data_dir();
        let lock = doze_runtime::DataDirLock::take(&dir, "doze-aws data").map_err(|e| match e {
            doze_runtime::LockError::Held { .. } => crate::startfail::data_dir_held(&dir),
            other => format!("doze-aws cannot use its data directory: {other}"),
        })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let tasks = doze_runtime::Tasks::new();
        let all: Vec<&'static crate::services::Svc> = crate::services::ALL.iter().collect();
        let gw = aws_gateway::Config::new(std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
        let (_gateway, _resume, data) = crate::wire::gateway(cfg, gw, &all, &tasks);
        Ok(Self::Local(Local {
            runtime,
            data,
            tasks,
            _lock: lock,
        }))
    }

    /// `GET /_doze/data`, or the engine's report.
    pub(crate) fn report(&self, only: Option<&str>) -> Result<Value, String> {
        match self {
            Self::Server(client, key) => {
                let path = only.map_or_else(
                    || "/_doze/data".to_owned(),
                    |s| format!("/_doze/data?service={s}"),
                );
                answer(client.hook(
                    &path,
                    None,
                    &headers(key.as_ref()),
                    Duration::from_secs(120),
                ))
            }
            Self::Local(l) => l.runtime.block_on(async {
                let parts = l.data.report(only).await.map_err(|r| r.message)?;
                Ok(data::to_json(l.data.dir(), l.data.total(), &parts))
            }),
        }
    }

    /// A removal: `POST /_doze/data/<what>` with `body`, or the engine's own.
    pub(crate) fn post(&self, what: &str, body: &Value) -> Result<Value, String> {
        match self {
            Self::Server(client, key) => {
                let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
                let path = format!("/_doze/data/{what}");
                answer(client.hook(
                    &path,
                    Some(&bytes),
                    &headers(key.as_ref()),
                    Duration::from_secs(600),
                ))
            }
            Self::Local(l) => l.runtime.block_on(local_post(&l.data, what, body)),
        }
    }

    pub(crate) fn close(self) {
        if let Self::Local(l) = self {
            // The services flush and close their files before the lock is let go.
            l.runtime.block_on(l.tasks.shutdown());
        }
    }
}

fn headers(key: Option<&String>) -> Vec<(&str, &str)> {
    key.map(|k| vec![("X-Doze-Key", k.as_str())])
        .unwrap_or_default()
}

fn answer(reply: Result<crate::client::Reply, String>) -> Result<Value, String> {
    let reply = reply?;
    let body: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
    if reply.status == 200 {
        Ok(body)
    } else {
        Err(body.get("error").and_then(Value::as_str).map_or_else(
            || format!("doze-aws answered {}", reply.status),
            str::to_owned,
        ))
    }
}

async fn local_post(data: &Data, what: &str, body: &Value) -> Result<Value, String> {
    match what {
        "empty" => {
            let target = target_of(body);
            data.empty(&target).await.map_err(|r| r.message)
        }
        "reset" => {
            let only = body.get("service").and_then(Value::as_str);
            let dry = body
                .get("dry_run")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let done = data.reset(only, dry).await.map_err(|r| r.message)?;
            Ok(data::reset_json(&done))
        }
        _ => {
            let freed = data.compact().await.map_err(|r| r.message)?;
            Ok(json!({"freed": freed}))
        }
    }
}

fn target_of(body: &Value) -> Target {
    let text = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_owned);
    Target {
        service: text("service").unwrap_or_default(),
        region: text("region"),
        name: text("name").unwrap_or_default(),
        prune: data::prune_of(body.get("prune").unwrap_or(&Value::Null)),
    }
}
