//! Temporal's capabilities beyond running: its declared namespaces (objects, converge, prune), attributes for other
//! blocks to reference, health for the supervisor, and the restart policy.

use crate::cli::Cli;
use crate::{
    Namespace, Temporal, TemporalSpec, UI, duration, first_namespace, program, tcp_backend,
};
use doze_module::caps::{Attributes, Converge, Health, Objects, Prune, Restarts};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Instance, Object, Toolchain};
use doze_module::proto::msg::run::Restart;
use doze_module::proto::msg::value::Value;
use doze_module::{Cx, Result, help};
use std::collections::BTreeMap;

const NAMESPACE: &str = "namespace";

/// The CLI against this instance's server, from `bin_dir` or the instance's own toolchain.
pub(crate) fn cli(instance: &Instance, bin_dir: Option<&str>) -> Result<Cli> {
    let dir = bin_dir
        .map(String::from)
        .or_else(|| instance.toolchain.as_ref().map(|t| t.bin_dir.clone()))
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "the instance has no toolchain yet"))?;
    Ok(Cli {
        program: program(&dir),
        address: tcp_backend(instance, "main")?,
    })
}

fn hash(ns: &Namespace) -> String {
    help::object_hash(&(
        ns.name.as_str(),
        ns.retention.as_deref(),
        ns.description.as_deref(),
    ))
}

impl Objects for Temporal {
    fn objects(&self, cx: &Cx<TemporalSpec>) -> Result<Vec<Object>> {
        let objects = cx.spec.namespace.iter().map(|ns| Object {
            kind: NAMESPACE.into(),
            name: ns.name.clone(),
            hash: hash(ns),
        });
        Ok(objects.collect())
    }
}

/// What differs between a declared namespace and the server's: the retention and description to set, if any.
fn changes<'a>(
    ns: &'a Namespace,
    has: &crate::cli::Described,
) -> (Option<&'a str>, Option<&'a str>) {
    let retention = ns
        .retention
        .as_deref()
        .filter(|r| duration::millis(r) != has.retention_ms);
    let description = ns.description.as_deref().filter(|d| *d != has.description);
    (retention, description)
}

impl Converge for Temporal {
    async fn converge(&self, cx: &Cx<TemporalSpec>, tc: &Toolchain) -> Result<()> {
        let cli = cli(&cx.instance, Some(&tc.bin_dir))?;
        for ns in &cx.spec.namespace {
            let (retention, description) = (ns.retention.as_deref(), ns.description.as_deref());
            match cli.describe(&ns.name).await? {
                None => {
                    cli.put("create", &ns.name, retention, description).await?;
                    cx.log(2, format!("namespace {}: created", ns.name));
                }
                Some(has) => {
                    let (retention, description) = changes(ns, &has);
                    if retention.is_none() && description.is_none() {
                        continue;
                    }
                    cli.put("update", &ns.name, retention, description).await?;
                    let what: Vec<&str> = [(retention, "retention"), (description, "description")]
                        .iter()
                        .filter_map(|(v, w)| v.map(|_| *w))
                        .collect();
                    cx.log(
                        2,
                        format!("namespace {}: {} updated", ns.name, what.join(" and ")),
                    );
                }
            }
        }
        Ok(())
    }
}

impl Prune for Temporal {
    async fn prune(&self, cx: &Cx<TemporalSpec>, tc: &Toolchain, removed: &[Object]) -> Result<()> {
        let cli = cli(&cx.instance, Some(&tc.bin_dir))?;
        for o in removed.iter().filter(|o| o.kind == NAMESPACE) {
            cli.delete(&o.name).await?;
            cx.log(2, format!("namespace {}: deleted", o.name));
        }
        Ok(())
    }
}

impl Attributes for Temporal {
    fn attributes(&self, cx: &Cx<TemporalSpec>) -> Result<BTreeMap<String, Value>> {
        let addr = |name: &str| {
            cx.instance
                .listeners
                .get(name)
                .map(|l| l.addr.clone())
                .unwrap_or_default()
        };
        let mut out = BTreeMap::from([
            ("address".to_string(), Value::String(addr("main"))),
            (
                "namespace".to_string(),
                Value::String(first_namespace(&cx.spec)),
            ),
        ]);
        if !cx.spec.headless {
            out.insert(
                "ui_url".into(),
                Value::String(format!("http://{}", addr(UI))),
            );
        }
        Ok(out)
    }
}

impl Health for Temporal {
    async fn health(&self, cx: &Cx<TemporalSpec>) -> Result<()> {
        cli(&cx.instance, None)?.health().await
    }
}

impl Restarts for Temporal {
    fn restart(&self, cx: &Cx<TemporalSpec>) -> Restart {
        // Go's defaults: on failure, after a second, five times.
        let r = cx.spec.restart.first();
        let policy = match r.and_then(|r| r.policy.as_deref()) {
            Some("no") => 0,
            Some("always") => 2,
            _ => 1,
        };
        let backoff = r
            .and_then(|r| duration::millis(r.backoff.as_deref()?))
            .unwrap_or(1_000);
        Restart {
            policy,
            backoff_ms: u32::try_from(backoff).unwrap_or(u32::MAX),
            max_retries: r.and_then(|r| r.max_retries).unwrap_or(5),
        }
    }
}
