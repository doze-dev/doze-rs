//! The temporal CLI from the engine's own build, against the server's frontend: health, namespaces, workflow counts.
//! Shapes checked on temporal 1.8.2 (`-o json`); `-n` exists from 1.0.0 (the positional namespace is deprecated and
//! prints a warning, so the Go module's form would break on a future CLI).

use crate::duration;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Result, help};
use serde_json::Value as Json;
use std::path::PathBuf;

/// One server, through its CLI.
#[derive(Debug, Clone)]
pub(crate) struct Cli {
    pub(crate) program: PathBuf,
    pub(crate) address: String,
}

/// A namespace as the server has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Described {
    pub(crate) retention_ms: Option<u64>,
    pub(crate) description: String,
    pub(crate) state: String,
}

fn text(v: &Json, path: &str) -> String {
    v.pointer(path)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_string()
}

fn json(out: &str) -> Result<Json> {
    serde_json::from_str(out).map_err(|e| {
        Error::new(
            ErrorKind::Unavailable,
            format!("temporal answered something unreadable: {e}"),
        )
    })
}

impl Cli {
    async fn run(&self, args: &[&str]) -> Result<String> {
        let mut full: Vec<&str> = args.to_vec();
        full.extend(["--address", &self.address]);
        help::run_tool(&self.program, &full).await
    }

    /// `SERVING`, or why not.
    pub(crate) async fn health(&self) -> Result<()> {
        let out = self.run(&["operator", "cluster", "health"]).await?;
        if out.contains("SERVING") {
            return Ok(());
        }
        Err(Error::new(
            ErrorKind::Unavailable,
            format!("temporal is not serving: {}", out.trim()),
        ))
    }

    /// The namespace, or `None` when the server says it is not found. Any other failure is an error: protocol 1
    /// read every failure as "absent" and then tried to create it (module bug 9).
    pub(crate) async fn describe(&self, name: &str) -> Result<Option<Described>> {
        let out = match self
            .run(&[
                "operator",
                "namespace",
                "describe",
                "-n",
                name,
                "-o",
                "json",
            ])
            .await
        {
            Ok(out) => out,
            Err(e) if e.message.contains("is not found") => return Ok(None),
            Err(e) => return Err(e),
        };
        let v = json(&out)?;
        Ok(Some(Described {
            retention_ms: duration::millis(&text(&v, "/config/workflowExecutionRetentionTtl")),
            description: text(&v, "/namespaceInfo/description"),
            state: text(&v, "/namespaceInfo/state"),
        }))
    }

    /// Creates or updates (`verb`) a namespace with what is declared.
    pub(crate) async fn put(
        &self,
        verb: &str,
        name: &str,
        retention: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        let mut args = vec!["operator", "namespace", verb, "-n", name];
        if let Some(r) = retention {
            args.extend(["--retention", r]);
        }
        if let Some(d) = description {
            args.extend(["--description", d]);
        }
        self.run(&args).await.map(drop)
    }

    pub(crate) async fn delete(&self, name: &str) -> Result<()> {
        self.run(&["operator", "namespace", "delete", "-n", name, "--yes"])
            .await
            .map(drop)
    }

    /// Every namespace's name, with its description and state.
    pub(crate) async fn list(&self) -> Result<Vec<(String, Described)>> {
        let v = json(
            &self
                .run(&["operator", "namespace", "list", "-o", "json"])
                .await?,
        )?;
        let items = v.as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .map(|n| {
                let described = Described {
                    retention_ms: duration::millis(&text(
                        n,
                        "/config/workflowExecutionRetentionTtl",
                    )),
                    description: text(n, "/namespaceInfo/description"),
                    state: text(n, "/namespaceInfo/state"),
                };
                (text(n, "/namespaceInfo/name"), described)
            })
            .collect())
    }

    /// Workflows running in `namespace` (the CLI leaves `count` out when it is zero).
    pub(crate) async fn running(&self, namespace: &str) -> Result<u64> {
        let out = self
            .run(&[
                "workflow",
                "count",
                "-n",
                namespace,
                "--query",
                "ExecutionStatus='Running'",
                "-o",
                "json",
            ])
            .await?;
        let v = json(&out)?;
        // protojson writes an int64 as a string; take a number too.
        let count = v.get("count");
        let parsed = count.and_then(|c| {
            c.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| c.as_u64())
        });
        Ok(parsed.unwrap_or(0))
    }
}
