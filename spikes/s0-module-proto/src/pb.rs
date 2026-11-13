//! Prost equivalents of the messages in `msg.rs` (hand-annotated, no .proto, no build
//! script), so prost can be compared on the same data. Only what the codec bench needs.

use std::collections::BTreeMap;

#[derive(Clone, PartialEq, prost::Message)]
pub struct Span {
    #[prost(uint32, tag = "1")] pub file: u32,
    #[prost(uint32, tag = "2")] pub start: u32,
    #[prost(uint32, tag = "3")] pub end: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct NodeList {
    #[prost(message, repeated, tag = "1")] pub items: Vec<Node>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct NodeMap {
    #[prost(btree_map = "string, message", tag = "1")] pub items: BTreeMap<String, Node>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
pub enum Value {
    #[prost(bool, tag = "2")] Null(bool),
    #[prost(bool, tag = "3")] Bool(bool),
    #[prost(string, tag = "4")] Num(String),
    #[prost(string, tag = "5")] Str(String),
    #[prost(message, tag = "6")] List(NodeList),
    #[prost(message, tag = "7")] Map(NodeMap),
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Node {
    #[prost(message, optional, tag = "1")] pub span: Option<Span>,
    #[prost(oneof = "Value", tags = "2, 3, 4, 5, 6, 7")] pub v: Option<Value>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Granted {
    #[prost(string, tag = "1")] pub addr: String,
    #[prost(string, tag = "2")] pub url: String,
    #[prost(string, repeated, tag = "3")] pub hosts: Vec<String>,
    #[prost(string, tag = "4")] pub backend: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Instance {
    #[prost(string, tag = "1")] pub name: String,
    #[prost(string, tag = "2")] pub ty: String,
    #[prost(string, tag = "3")] pub version: String,
    #[prost(string, tag = "4")] pub data_dir: String,
    #[prost(string, tag = "5")] pub socket_dir: String,
    #[prost(message, optional, tag = "6")] pub spec: Option<Node>,
    #[prost(string, tag = "7")] pub host: String,
    #[prost(btree_map = "string, message", tag = "8")] pub listeners: BTreeMap<String, Granted>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Toolchain {
    #[prost(string, tag = "1")] pub engine: String,
    #[prost(string, tag = "2")] pub full: String,
    #[prost(string, tag = "3")] pub bin_dir: String,
    #[prost(btree_map = "string, string", tag = "4")] pub tools: BTreeMap<String, String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct PlanReq {
    #[prost(message, optional, tag = "1")] pub inst: Option<Instance>,
    #[prost(message, optional, tag = "2")] pub tc: Option<Toolchain>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Ready {
    #[prost(string, tag = "1")] pub kind: String,
    #[prost(string, tag = "2")] pub target: String,
    #[prost(uint32, tag = "3")] pub interval_ms: u32,
    #[prost(uint32, tag = "4")] pub timeout_ms: u32,
    #[prost(uint32, tag = "5")] pub retries: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SpawnSpec {
    #[prost(string, tag = "1")] pub name: String,
    #[prost(string, tag = "2")] pub dir: String,
    #[prost(string, tag = "3")] pub bin: String,
    #[prost(string, repeated, tag = "4")] pub args: Vec<String>,
    #[prost(string, repeated, tag = "5")] pub env: Vec<String>,
    #[prost(bool, tag = "6")] pub tree: bool,
    #[prost(string, repeated, tag = "7")] pub after: Vec<String>,
    #[prost(message, optional, tag = "8")] pub ready: Option<Ready>,
    #[prost(string, repeated, tag = "9")] pub hooks: Vec<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SpawnPlan {
    #[prost(message, repeated, tag = "1")] pub specs: Vec<SpawnSpec>,
}

// ---- conversions from the msg fixtures (bench setup only) ---------------------

use crate::msg as m;

fn span(s: m::Span) -> Span {
    Span { file: u32::from(s.file), start: s.start, end: s.end }
}

pub fn node(n: &m::Node) -> Node {
    let v = match &n.v {
        m::Value::Null => Value::Null(true),
        m::Value::Bool(b) => Value::Bool(*b),
        m::Value::Num(x) => Value::Num(x.clone()),
        m::Value::Str(x) => Value::Str(x.clone()),
        m::Value::List(xs) => Value::List(NodeList { items: xs.iter().map(node).collect() }),
        m::Value::Map(xs) => Value::Map(NodeMap { items: xs.iter().map(|(k, v)| (k.clone(), node(v))).collect() }),
    };
    Node { span: Some(span(n.span)), v: Some(v) }
}

pub fn plan_req(r: &m::PlanReq) -> PlanReq {
    let i = &r.inst;
    PlanReq {
        inst: Some(Instance {
            name: i.name.clone(),
            ty: i.ty.clone(),
            version: i.version.clone(),
            data_dir: i.data_dir.clone(),
            socket_dir: i.socket_dir.clone(),
            spec: Some(node(&i.spec)),
            host: i.host.clone(),
            listeners: i
                .listeners
                .iter()
                .map(|(k, g)| {
                    (k.clone(), Granted { addr: g.addr.clone(), url: g.url.clone(), hosts: g.hosts.clone(), backend: g.backend.clone() })
                })
                .collect(),
        }),
        tc: Some(Toolchain {
            engine: r.tc.engine.clone(),
            full: r.tc.full.clone(),
            bin_dir: r.tc.bin_dir.clone(),
            tools: r.tc.tools.clone(),
        }),
    }
}

pub fn spawn_plan(p: &m::SpawnPlan) -> SpawnPlan {
    SpawnPlan {
        specs: p
            .specs
            .iter()
            .map(|s| SpawnSpec {
                name: s.name.clone(),
                dir: s.dir.clone(),
                bin: s.bin.clone(),
                args: s.args.clone(),
                env: s.env.clone(),
                tree: s.tree,
                after: s.after.clone(),
                ready: s.ready.as_ref().map(|r| Ready {
                    kind: r.kind.clone(),
                    target: r.target.clone(),
                    interval_ms: r.interval_ms,
                    timeout_ms: r.timeout_ms,
                    retries: r.retries,
                }),
                hooks: s.hooks.clone(),
            })
            .collect(),
    }
}
