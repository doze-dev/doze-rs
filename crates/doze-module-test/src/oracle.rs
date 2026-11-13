//! Comparing a Rust module with the Go module it replaces (PLAN.md step 5). `tools/module-oracle` ran the Go modules on
//! a corpus and recorded their answers (`golden.jsonl`); this asks the Rust module the same questions about the same
//! fixed instance and reports every difference that is not a declared, reasoned deviation.
//!
//! Compared: whether the block is refused (not the wording), the connection variable and URL, the objects (kind and
//! name; hashes are defined anew), and the plan: each process's program, arguments, added environment, order and
//! readiness check.

use crate::hcl;
use doze_module_host::Running;
use doze_module_proto::msg::instance::Toolchain;
use doze_module_proto::msg::instance::{Address, Granted, Instance, Listener};
use doze_module_proto::msg::run::{Plan, Ready, WithToolchain};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

/// A field of a JSON object, or null.
fn f<'a>(v: &'a Json, k: &str) -> &'a Json {
    v.get(k).unwrap_or(&Json::Null)
}

fn set(v: &mut Json, k: &str, x: Json) {
    if let Some(o) = v.as_object_mut() {
        o.insert(k.to_string(), x);
    }
}

/// Where the fixed instance's directories are (the Go oracle's `root`).
const ROOT: &str = "/tmp/dzo";

fn ready(r: &Option<Ready>) -> String {
    match r {
        None => String::new(),
        Some(Ready::Tcp(a)) => format!("tcp {a}"),
        Some(Ready::Socket(p)) => format!("socket {p}"),
        Some(Ready::Http(u)) => format!("http {u}"),
        Some(Ready::Exec(argv)) => format!("exec {}", argv.join(" ")),
        Some(Ready::LogLine(l)) => format!("log_line {l}"),
    }
}

fn plan_json(p: &Plan) -> Json {
    Json::Array(
        p.processes
            .iter()
            .map(|pr| {
                json!({
                    "name": pr.name, "program": pr.program, "args": pr.args, "env_added": pr.env,
                    "after": pr.after, "ready": ready(&pr.ready),
                    "hooks": pr.hooks.iter().map(|h| h.join(" ")).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// The instance the Go oracle used, rebuilt for one case.
fn instance(case: &Json, parsed: hcl::Parsed, backend: &str) -> Instance {
    let name = parsed.name.clone();
    let main = Granted {
        addr: "127.0.0.1:7777".into(),
        url: "tcp://127.0.0.1:7777".into(),
        hosts: vec![format!("{name}.shop.doze")],
        backend: Address::Unix(backend.to_string()),
    };
    Instance {
        engine: parsed.engine,
        version: f(case, "version").as_str().unwrap_or_default().to_string(),
        data_dir: format!("{ROOT}/d/{name}"),
        socket_dir: format!("{ROOT}/s/{name}"),
        spec: parsed.spec,
        host: format!("{name}.shop.doze"),
        listeners: BTreeMap::from([("main".to_string(), main)]),
        deps: vec![],
        toolchain: None,
        name,
    }
}

/// Grants each listener that needs TCP (`Listener::tcp`) the way a protocol-1 host did: the engine listened on its
/// conventional port itself (the instance's `port` for "main", its own `port` for the others). Go modules of that kind
/// (temporal) bound those ports directly, so this is the grant under which their answers are comparable.
fn regrant(inst: &mut Instance, listeners: &[Listener], main_port: u16) {
    for l in listeners.iter().filter(|l| l.tcp) {
        let main = l.name == doze_module_proto::network::MAIN;
        let port = if main { main_port } else { l.port };
        let backend = format!("127.0.0.1:{port}");
        let addr = if main {
            "127.0.0.1:7777".to_string()
        } else {
            backend.clone()
        };
        let scheme = if l.http { "http" } else { "tcp" };
        let granted = Granted {
            url: format!("{scheme}://{addr}"),
            addr,
            hosts: vec![],
            backend: Address::Tcp(backend),
        };
        inst.listeners.insert(l.name.clone(), granted);
    }
}

/// The Rust module's answers for one case, in the oracle's shape.
async fn answer(m: &Running, case: &Json, golden: &Json) -> Json {
    let src = f(case, "hcl").as_str().unwrap_or_default();
    let parsed = match hcl::parse_block(src, &m.manifest.schema) {
        Ok(p) => p,
        Err(e) => return json!({ "decode_error": e.to_string() }),
    };
    let backend = f(golden, "backend").as_str().unwrap_or_default();
    let main_port = parsed.port.unwrap_or(m.manifest.default_port);
    let mut inst = instance(case, parsed, backend);
    if let Err(e) = m.decode(&inst).await {
        return json!({ "decode_error": e.message });
    }
    if let Ok(listeners) = m.network(&inst).await {
        regrant(&mut inst, &listeners, main_port);
    }
    let mut out = json!({ "backend": backend });
    if let Ok(c) = m.connection(&inst).await {
        set(&mut out, "env_var", json!(c.env_var));
        set(&mut out, "url", json!(c.url));
    }
    if let Ok(objs) = m.objects(&inst).await {
        let objects = Json::Array(
            objs.iter()
                .map(|o| json!({ "kind": o.kind, "name": o.name }))
                .collect(),
        );
        set(&mut out, "objects", objects);
    }
    let toolchain = Toolchain {
        engine: inst.engine.clone(),
        full: "9.9.9".into(),
        bin_dir: "/tc/bin".into(),
        tools: BTreeMap::new(),
    };
    match m
        .plan(&WithToolchain {
            instance: inst,
            toolchain,
        })
        .await
    {
        Ok(p) => set(&mut out, "plan", plan_json(&p)),
        Err(e) => set(&mut out, "plan_error", json!(e.message)),
    }
    out
}

/// Paths with forward slashes: the Go answers were recorded on Unix, and on Windows a Rust module joins paths with `\`.
/// The comparison is about what the module does, not the separator of the system it runs on.
fn slash(v: &Json) -> Json {
    match v {
        Json::String(s) => Json::String(s.replace('\\', "/")),
        Json::Array(items) => Json::Array(items.iter().map(slash).collect()),
        other => other.clone(),
    }
}

/// A program path as on Unix: forward slashes, and no `.exe` (on Windows a module names `kvrocks.exe`, rightly).
fn program(v: &Json) -> Json {
    match slash(v) {
        Json::String(s) => Json::String(s.strip_suffix(".exe").unwrap_or(&s).to_string()),
        other => other,
    }
}

/// A readiness check as on Unix: an `exec` check names a program too, and on Windows that program carries `.exe`.
fn ready_command(v: &Json) -> Json {
    match slash(v) {
        Json::String(s) => Json::String(s.replacen(".exe ", " ", 1)),
        other => other,
    }
}

/// The parts of a Go answer that must match (wording, hashes and nulls aside).
fn comparable(a: &Json) -> Json {
    if a.get("decode_error").is_some() {
        return json!({ "refused": true });
    }
    let objects: Vec<Json> = f(a, "objects")
        .as_array()
        .map(|v| {
            v.iter()
                .map(|o| json!({ "kind": f(o, "kind"), "name": f(o, "name") }))
                .collect()
        })
        .unwrap_or_default();
    let plan: Vec<Json> = f(a, "plan").as_array().map(|ps| ps.iter().map(|p| {
        let list = |k: &str| f(p, k).as_array().cloned().unwrap_or_default();
        json!({ "name": f(p, "name"), "program": program(f(p, "program")), "args": slash(&Json::Array(list("args"))),
                "env_added": f(p, "env_added"), "after": list("after"), "ready": ready_command(f(p, "ready")), "hooks": list("hooks") })
    }).collect()).unwrap_or_default();
    json!({ "refused": false, "env_var": f(a, "env_var"), "url": f(a, "url"), "objects": objects, "plan": plan,
            "plan_error": a.get("plan_error").is_some() })
}

/// One difference between Go and Rust on a case.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    pub id: String,
    pub go: Json,
    pub rust: Json,
}

/// Runs every corpus case of `engine` through the Rust module and returns the differences not listed in `deviations`
/// (case ids the module documents as deliberate, each with its reason, e.g. a Go bug fixed).
pub async fn compare(
    m: &Running,
    engine: &str,
    corpus: &str,
    golden: &str,
    deviations: &[&str],
) -> Vec<Difference> {
    let golden: BTreeMap<String, Json> = golden
        .lines()
        .filter_map(|l| serde_json::from_str::<Json>(l).ok())
        .filter_map(|g| f(&g, "id").as_str().map(|id| (id.to_string(), g.clone())))
        .collect();
    let mut diffs = Vec::new();
    for line in corpus.lines() {
        let Ok(case) = serde_json::from_str::<Json>(line) else {
            continue;
        };
        let id = f(&case, "id").as_str().unwrap_or_default().to_string();
        if f(&case, "engine").as_str() != Some(engine) || deviations.contains(&id.as_str()) {
            continue;
        }
        let Some(g) = golden.get(&id) else { continue };
        let rust = answer(m, &case, g).await;
        let (go, rust) = (comparable(g), comparable(&rust));
        if go != rust {
            diffs.push(Difference { id, go, rust });
        }
    }
    diffs
}
