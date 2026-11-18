//! The oracle's machine and modules, and a loaded config as Go's `json.Marshal` writes it.

#![allow(dead_code, unreachable_pub)]

use doze_config::model::{Command, Config, Instance, ModuleBody, Probe, Sleep, Spec};
use doze_config::modules::{ModuleInfo, Modules};
use doze_config::{Host, Kind};
use doze_hcl::Value;
use doze_hcl::types::Type;
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The oracle's machine: only `PATH` and `HOME` are set.
pub struct OracleHost {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub zone: Option<String>,
}

impl Host for OracleHost {
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        let home = self.home.to_string_lossy().into_owned();
        let zone = self.zone.clone();
        Arc::new(move |name: &str| match name {
            "PATH" => Some("/usr/bin:/bin".into()),
            "HOME" => Some(home.clone()),
            "DOZE_ZONE" => zone.clone(),
            _ => None,
        })
    }
    fn cwd(&self) -> PathBuf {
        self.cwd.clone()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        Some(self.home.clone())
    }
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        doze_names::domains::valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        doze_names::domains::route_domain(host)
    }
    fn check_certificate(&self, _cert: &Path, _key: &Path, _domain: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The oracle's module types (tools/oracle-dump/config.go), with the ports Go's
/// `detect.ConventionalPort` gives them (temporal is the exception: it takes none).
pub struct Fixtures;

const TYPES: [(&str, u16, bool); 11] = [
    ("fake", 0, false),
    ("postgres", 5432, false),
    ("mariadb", 3306, false),
    ("valkey", 6379, false),
    ("kvrocks", 6380, false),
    ("temporal", 0, false),
    ("ferret", 27017, false),
    ("kafka", 9092, false),
    ("standin", 0, false),
    ("aws", 4566, true),
    ("httpd", 0, true),
];

impl Modules for Fixtures {
    fn lookup(&self, ty: &str, _version: &str) -> Option<ModuleInfo> {
        self.cached(ty)
    }
    fn cached(&self, ty: &str) -> Option<ModuleInfo> {
        TYPES
            .iter()
            .find(|(t, ..)| *t == ty)
            .map(|(_, port, versionless)| ModuleInfo {
                versionless: *versionless,
                default_port: *port,
                supervised: false,
            })
    }
    fn names(&self) -> Vec<String> {
        TYPES.iter().map(|(t, ..)| (*t).to_owned()).collect()
    }
}

fn ns(d: std::time::Duration) -> Json {
    json!(u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

fn strs(v: &[String]) -> Json {
    if v.is_empty() { Json::Null } else { json!(v) }
}

fn map(m: &std::collections::BTreeMap<String, String>) -> Json {
    if m.is_empty() { Json::Null } else { json!(m) }
}

fn sleep(s: &Sleep) -> Json {
    json!({"Set": s.set, "Never": s.never, "After": ns(s.after)})
}

fn probe(p: Option<&Probe>) -> Json {
    p.map_or(Json::Null, |p| json!({"Kind": p.kind, "Arg": p.arg}))
}

fn refs(rs: &[doze_config::Ref]) -> Json {
    Json::Array(
        rs.iter()
            .map(|r| json!({"Kind": r.kind.as_str(), "Name": r.name}))
            .collect(),
    )
}

fn command(c: &Command) -> Json {
    json!({
        "Dir": c.dir, "Run": c.run, "Port": 0, "Env": map(&c.env), "EnvFile": c.env_file,
        "Setup": strs(&c.setup), "OnReady": strs(&c.on_ready), "Stop": strs(&c.stop),
        "Ready": probe(c.ready.as_ref()), "ReadyTimeout": ns(c.ready_timeout),
        "Restart": {"Policy": c.restart.policy, "Backoff": ns(c.restart.backoff), "MaxRetries": c.restart.max_retries},
        "Sleep": sleep(&c.sleep), "Console": c.console, "Watch": strs(&c.watch), "Direct": c.direct,
        "Download": map(&c.download), "Service": c.service, "Task": c.task, "Inputs": strs(&c.inputs), "Manual": c.manual,
    })
}

/// cty's JSON type (`ctyjson.MarshalType`).
pub fn type_json(t: &Type) -> Json {
    match t {
        Type::Dynamic => json!("dynamic"),
        Type::String => json!("string"),
        Type::Number => json!("number"),
        Type::Bool => json!("bool"),
        Type::List(e) => json!(["list", type_json(e)]),
        Type::Set(e) => json!(["set", type_json(e)]),
        Type::Map(e) => json!(["map", type_json(e)]),
        Type::Tuple(es) => json!(["tuple", es.iter().map(type_json).collect::<Vec<_>>()]),
        Type::Object(o) => {
            let attrs: serde_json::Map<String, Json> = o
                .attrs
                .iter()
                .map(|(k, t)| (k.clone(), type_json(t)))
                .collect();
            if o.optional.is_empty() {
                json!(["object", attrs])
            } else {
                json!(["object", attrs, o.optional.iter().collect::<Vec<_>>()])
            }
        }
    }
}

/// The oracle's `canon`.
pub fn canon(v: &Value) -> Json {
    match v {
        Value::Unknown(..) => json!({"unknown": true}),
        Value::Null(_) => Json::Null,
        Value::String(s) => json!(s),
        Value::Bool(b) => json!(b),
        Value::Number(n) => json!({"num": n.text()}),
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => {
            Json::Array(es.iter().map(canon).collect())
        }
        Value::Map(_, m) | Value::Object(m) => {
            json!({"obj": m.iter().map(|(k, v)| json!([k, canon(v)])).collect::<Vec<_>>()})
        }
    }
}

fn module(b: &ModuleBody) -> Json {
    let attrs: serde_json::Map<String, Json> = b
        .attrs
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                json!({"value": canon(v), "type": type_json(&v.ty())}),
            )
        })
        .collect();
    let blocks: Vec<Json> = b
        .blocks
        .iter()
        .map(|bl| json!({"type": bl.ty, "labels": if bl.labels.is_empty() { Json::Null } else { json!(bl.labels) }, "body": module(&bl.body)}))
        .collect();
    json!({"attrs": attrs, "blocks": blocks})
}

fn instance(i: &Instance) -> Json {
    let spec = match &i.spec {
        Spec::None => Json::Null,
        Spec::Command(c) => command(c),
        Spec::Module(b) => module(b),
    };
    json!({
        "Kind": i.kind().as_str(), "Type": i.ty, "Name": i.name, "Version": i.version, "Listen": i.listen,
        "Port": i.port, "Domains": strs(&i.domains), "Cert": i.cert, "Key": i.key, "Spec": spec, "Index": i.index,
        "Deps": Json::Array(i.deps.iter().map(|d| json!({"Name": d.name, "Condition": d.condition.as_str()})).collect()),
        "Enabled": i.enabled, "After": refs(&i.after), "Refs": refs(&i.refs), "Sleep": sleep(&i.sleep), "NoRoutes": i.no_routes,
    })
}

/// A config as Go's JSON.
pub fn config_json(c: &Config) -> Json {
    let d = &c.defaults;
    let services: Vec<Json> = c.services.iter().map(|s| json!({
        "Name": s.name, "Run": s.run, "Dir": s.dir, "Port": s.port, "DirSet": s.dir_set, "Env": map(&s.env), "EnvFile": s.env_file,
        "Download": map(&s.download), "Setup": strs(&s.setup), "OnReady": strs(&s.on_ready), "Stop": strs(&s.stop),
        "Ready": probe(s.ready.as_ref()), "ReadyTimeout": ns(s.ready_timeout),
        "Restart": {"Policy": s.restart.policy, "Backoff": ns(s.restart.backoff), "MaxRetries": s.restart.max_retries},
        "Sleep": sleep(&s.sleep), "Console": s.console, "After": refs(&s.after), "Refs": refs(&s.refs),
    })).collect();
    let tasks: Vec<Json> = c.tasks.iter().map(|t| json!({
        "Name": t.name, "Run": t.run, "Dir": t.dir, "Env": map(&t.env), "EnvFile": t.env_file, "Inputs": strs(&t.inputs),
        "Manual": t.manual, "After": refs(&t.after), "Refs": refs(&t.refs),
    })).collect();
    let outputs: serde_json::Map<String, Json> = c.outputs.iter().map(|(k, o)| (k.clone(), json!({
        "Name": o.name, "Value": o.value, "Description": o.description, "Sensitive": o.sensitive,
    }))).collect();
    json!({
        "Warnings": strs(&c.warnings), "Listen": c.listen, "Home": c.home, "DataDir": c.data_dir, "StackName": c.stack_name,
        "Defaults": {"IdleTimeout": ns(d.idle_timeout), "Domains": d.names, "LogRuns": d.log_runs, "LogBytes": d.log_bytes,
            "Commands": {"ReadyTimeout": ns(d.commands.ready_timeout), "RestartPolicy": d.commands.restart_policy,
                "RestartBackoff": ns(d.commands.restart_backoff), "RestartMaxRetries": d.commands.restart_max_retries, "Env": map(&d.commands.env)}},
        "TLS": {"Enabled": c.tls.enabled, "Cert": c.tls.cert, "Key": c.tls.key, "Required": c.tls.required},
        "Instances": c.instances.iter().map(instance).collect::<Vec<_>>(),
        "Services": services, "Tasks": tasks, "Outputs": outputs, "OutputOrder": strs(&c.output_order),
    })
}

/// Go's JSON and ours made comparable: null, `[]` and `{}` are one; `Deps` and `Refs` sorted (Go
/// collects them in a map's order).
pub fn normalize(v: Json) -> Json {
    match v {
        Json::Array(a) if a.is_empty() => Json::Null,
        Json::Object(o) if o.is_empty() => Json::Null,
        Json::Array(a) => Json::Array(a.into_iter().map(normalize).collect()),
        Json::Object(o) => Json::Object(
            o.into_iter()
                .map(|(k, v)| {
                    let mut v = normalize(v);
                    if (k == "Deps" || k == "Refs")
                        && let Json::Array(a) = &mut v
                    {
                        a.sort_by_key(|x| x.to_string());
                    }
                    (k, v)
                })
                .collect(),
        ),
        v => v,
    }
}

pub fn kind_str(k: Kind) -> &'static str {
    k.as_str()
}
