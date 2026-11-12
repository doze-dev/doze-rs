//! Helpers for the echo module tests: value builders, an instance, the host's fetch side, starting the module.
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used)]

use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::Unit;
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::hello::Hello;
use doze_module_proto::msg::instance::Instance;
use doze_module_proto::msg::run::Ensured;
use doze_module_proto::msg::value::{Node, PathSeg, Value};
use doze_module_proto::peer::{self, CallOpts, Handler, Incoming, Peer};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub fn s(v: &str) -> Node {
    Node::bare(Value::String(v.into()))
}
pub fn n(v: u32) -> Node {
    Node::bare(Value::Number(v.to_string()))
}
pub fn map(entries: &[(&str, Node)]) -> Node {
    Node::bare(Value::Map(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    ))
}
pub fn list(items: Vec<Node>) -> Node {
    Node::bare(Value::List(items))
}

pub fn instance(version: &str, spec: Node, data: &str) -> Instance {
    Instance {
        name: "e".into(),
        engine: "echo".into(),
        version: version.into(),
        data_dir: data.into(),
        socket_dir: "/run/e".into(),
        spec,
        host: String::new(),
        listeners: BTreeMap::new(),
        deps: vec![],
        toolchain: None,
    }
}

pub fn good() -> Node {
    let ch = |name: &str, size: u32| map(&[("name", s(name)), ("size", n(size))]);
    map(&[
        ("greeting", s("hi")),
        ("channel", list(vec![ch("a", 4), ch("b", 2)])),
    ])
}

/// The host's side of `fetch.*`, counting calls.
pub fn host(majors: Arc<AtomicU32>) -> Handler {
    Arc::new(move |c: Incoming| {
        let majors = majors.clone();
        Box::pin(async move {
            match c.method {
                Method::FetchResolveMajor => {
                    majors.fetch_add(1, Ordering::SeqCst);
                    drop(c.responder.ok(&"2.4.1".to_string()));
                }
                _ => drop(c.responder.ok(&Ensured {
                    bin_dir: "/cache/echo/bin".into(),
                    digest: "sha256:abc".into(),
                })),
            }
        })
    })
}

pub async fn start(majors: Arc<AtomicU32>) -> (Peer, Hello, tokio::process::Child) {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_doze-module-echo"));
    cmd.kill_on_drop(true);
    let (child, ch) = doze_os::channel::spawn_module(cmd).await.unwrap();
    let (peer, hello) = peer::accept_module(ch, host(majors), None).await.unwrap();
    (peer, hello, child)
}

pub async fn decode(p: &Peer, inst: &Instance) -> Result<Unit, Error> {
    p.call(Method::Decode, inst, CallOpts::default()).await
}

pub fn has(e: &Error, path: &[PathSeg], text: &str) -> bool {
    e.diagnostics
        .iter()
        .any(|d| d.path == path && d.message.contains(text))
}
