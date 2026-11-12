//! Representative protocol messages. Shapes follow docs/port/modules/PLAN.md D3/D4:
//! a hello with a manifest, an instance carrying an evaluated `Value` spec with spans,
//! a SpawnPlan, the fetch callback, console rows. Each type derives minicbor (numbered
//! fields) and serde (for postcard). Prost equivalents live in `pb.rs`.

use minicbor::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Hello {
    #[n(0)] pub protocol: u32,
    #[n(1)] pub module: String,
    #[n(2)] pub version: String,
    #[n(3)] pub caps: Vec<String>,
    #[n(4)] pub manifest: Manifest,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Manifest {
    #[n(0)] pub ty: String,
    #[n(1)] pub category: String,
    #[n(2)] pub versions: Vec<String>,
    #[n(3)] pub default_port: u16,
    #[n(4)] pub boot_budget_ms: u64,
    #[n(5)] pub backend_socket: String,
    #[n(6)] pub supervised: bool,
    #[n(7)] pub schema: Vec<Attr>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Attr {
    #[n(0)] pub name: String,
    #[n(1)] pub ty: String,
    #[n(2)] pub default: Option<String>,
    #[n(3)] pub doc: String,
    #[n(4)] pub required: bool,
    #[n(5)] pub since: Option<String>,
    #[n(6)] pub until: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Span {
    #[n(0)] pub file: u16,
    #[n(1)] pub start: u32,
    #[n(2)] pub end: u32,
}

/// An evaluated config value, as the core would send it (D1).
#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub enum Value {
    #[n(0)] Null,
    #[n(1)] Bool(#[n(0)] bool),
    /// Numbers travel as their exact decimal text (cty numbers are 512-bit).
    #[n(2)] Num(#[n(0)] String),
    #[n(3)] Str(#[n(0)] String),
    #[n(4)] List(#[n(0)] Vec<Node>),
    #[n(5)] Map(#[n(0)] BTreeMap<String, Node>),
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Node {
    #[n(0)] pub span: Span,
    #[n(1)] pub v: Value,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Granted {
    #[n(0)] pub addr: String,
    #[n(1)] pub url: String,
    #[n(2)] pub hosts: Vec<String>,
    #[n(3)] pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Instance {
    #[n(0)] pub name: String,
    #[n(1)] pub ty: String,
    #[n(2)] pub version: String,
    #[n(3)] pub data_dir: String,
    #[n(4)] pub socket_dir: String,
    #[n(5)] pub spec: Node,
    #[n(6)] pub host: String,
    #[n(7)] pub listeners: BTreeMap<String, Granted>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Toolchain {
    #[n(0)] pub engine: String,
    #[n(1)] pub full: String,
    #[n(2)] pub bin_dir: String,
    #[n(3)] pub tools: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct PlanReq {
    #[n(0)] pub inst: Instance,
    #[n(1)] pub tc: Toolchain,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Ready {
    #[n(0)] pub kind: String,
    #[n(1)] pub target: String,
    #[n(2)] pub interval_ms: u32,
    #[n(3)] pub timeout_ms: u32,
    #[n(4)] pub retries: u32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct SpawnSpec {
    #[n(0)] pub name: String,
    #[n(1)] pub dir: String,
    #[n(2)] pub bin: String,
    #[n(3)] pub args: Vec<String>,
    #[n(4)] pub env: Vec<String>,
    #[n(5)] pub tree: bool,
    #[n(6)] pub after: Vec<String>,
    #[n(7)] pub ready: Option<Ready>,
    #[n(8)] pub hooks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct SpawnPlan {
    #[n(0)] pub specs: Vec<SpawnSpec>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct EnsureReq {
    #[n(0)] pub engine: String,
    #[n(1)] pub full: String,
    #[n(2)] pub triple: String,
    #[n(3)] pub expected_sha: String,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct EnsureResp {
    #[n(0)] pub bin_dir: String,
    #[n(1)] pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct TailReq {
    #[n(0)] pub panel: String,
    #[n(1)] pub selector: String,
    /// 0 = stream forever (until cancelled).
    #[n(2)] pub rows: u32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Row {
    #[n(0)] pub cells: BTreeMap<String, String>,
    #[n(1)] pub tone: String,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct WireAccept {
    #[n(0)] pub require_tls: bool,
    #[n(1)] pub local_unix: bool,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode, Serialize, Deserialize)]
pub struct Text {
    #[n(0)] pub s: String,
}

// ---- fixtures -------------------------------------------------------------------

fn sp(start: u32, end: u32) -> Span {
    Span { file: 0, start, end }
}

fn s(x: &str) -> String {
    x.to_string()
}

fn leaf(v: Value, at: u32) -> Node {
    Node { span: sp(at, at + 8), v }
}

/// A postgres-sized spec: settings map, two roles, one grant, three extensions.
pub fn sample_spec() -> Node {
    let mut settings = BTreeMap::new();
    settings.insert(s("work_mem"), leaf(Value::Str(s("64MB")), 40));
    settings.insert(s("log_statement"), leaf(Value::Str(s("all")), 60));
    let role = |name: &str, at: u32| {
        let mut m = BTreeMap::new();
        m.insert(s("name"), leaf(Value::Str(s(name)), at));
        m.insert(s("login"), leaf(Value::Bool(true), at + 10));
        m.insert(s("password"), leaf(Value::Str(s("s3cret-password")), at + 20));
        m.insert(s("connection_limit"), leaf(Value::Num(s("-1")), at + 30));
        m.insert(
            s("member_of"),
            Node { span: sp(at + 40, at + 60), v: Value::List(vec![leaf(Value::Str(s("readers")), at + 41)]) },
        );
        Node { span: sp(at, at + 80), v: Value::Map(m) }
    };
    let mut top = BTreeMap::new();
    top.insert(s("owner"), leaf(Value::Str(s("app")), 10));
    top.insert(s("shared_buffers"), leaf(Value::Str(s("128MB")), 20));
    top.insert(s("max_connections"), leaf(Value::Num(s("100")), 30));
    top.insert(s("settings"), Node { span: sp(35, 90), v: Value::Map(settings) });
    top.insert(
        s("role"),
        Node { span: sp(100, 300), v: Value::List(vec![role("app", 100), role("reader", 200)]) },
    );
    top.insert(
        s("extensions"),
        Node {
            span: sp(310, 360),
            v: Value::List(vec![
                leaf(Value::Str(s("vector")), 311),
                leaf(Value::Str(s("pg_cron")), 321),
                leaf(Value::Str(s("pg_trgm")), 331),
            ]),
        },
    );
    Node { span: sp(0, 400), v: Value::Map(top) }
}

pub fn sample_plan_req() -> PlanReq {
    let mut listeners = BTreeMap::new();
    listeners.insert(
        s("main"),
        Granted {
            addr: s("127.0.0.1:5432"),
            url: s("postgres://db.shop.doze:5432"),
            hosts: vec![s("db.shop.doze")],
            backend: s("/Users/dev/.doze/projects/shop-1a2b3c/run/db/main.sock"),
        },
    );
    PlanReq {
        inst: Instance {
            name: s("db"),
            ty: s("postgres"),
            version: s("17"),
            data_dir: s("/Users/dev/.doze/projects/shop-1a2b3c/data/db"),
            socket_dir: s("/Users/dev/.doze/projects/shop-1a2b3c/run/db"),
            spec: sample_spec(),
            host: s("db.shop.doze"),
            listeners,
        },
        tc: Toolchain {
            engine: s("postgres"),
            full: s("17.6.0"),
            bin_dir: s("/Users/dev/.doze/postgres/17.6.0-aarch64-apple-darwin/bin"),
            tools: BTreeMap::new(),
        },
    }
}

pub fn sample_spawn_plan() -> SpawnPlan {
    SpawnPlan {
        specs: vec![SpawnSpec {
            name: s("postgres"),
            dir: s(""),
            bin: s("/Users/dev/.doze/postgres/17.6.0-aarch64-apple-darwin/bin/postgres"),
            args: ["-D", "/Users/dev/.doze/projects/shop-1a2b3c/data/db", "-k", "/Users/dev/.doze/projects/shop-1a2b3c/run/db", "-p", "5432", "-c", "listen_addresses="]
                .iter()
                .map(|x| s(x))
                .collect(),
            env: vec![s("PGTZ=UTC"), s("LC_ALL=C")],
            tree: false,
            after: vec![],
            ready: Some(Ready {
                kind: s("exec"),
                target: s("pg_isready -h /Users/dev/.doze/projects/shop-1a2b3c/run/db -p 5432 -d postgres -U postgres"),
                interval_ms: 100,
                timeout_ms: 30_000,
                retries: 300,
            }),
            hooks: vec![],
        }],
    }
}

pub fn sample_hello() -> Hello {
    let attr = |name: &str, ty: &str| Attr {
        name: s(name),
        ty: s(ty),
        default: Some(s("x")),
        doc: s("A one-line description of what this argument does."),
        required: false,
        since: None,
        until: None,
    };
    Hello {
        protocol: 2,
        module: s("postgres"),
        version: s("0.3.0"),
        caps: ["converge", "objects", "prune", "admin", "console", "wire", "template"].iter().map(|x| s(x)).collect(),
        manifest: Manifest {
            ty: s("postgres"),
            category: s("database"),
            versions: ["14", "15", "16", "17", "18"].iter().map(|x| s(x)).collect(),
            default_port: 5432,
            boot_budget_ms: 0,
            backend_socket: s(".s.PGSQL.{port}"),
            supervised: false,
            schema: (0..24).map(|i| attr(&format!("arg_{i}"), "string")).collect(),
        },
    }
}
