//! The echo module, started as a real process through doze-os, driven by a host peer: the manifest the derive built,
//! decoding with diagnostics at the right path, the required calls, a fetch callback, and capability gating.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::method::Method;
use doze_module_proto::msg::calls::Unit;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::{Hello, caps};
use doze_module_proto::msg::instance::{Connection, Instance, Listener, Object, Toolchain};
use doze_module_proto::msg::run::{
    Ensured, Pin, Plan, Provisioned, Ready, ResolveArgs, Resolved, WithToolchain,
};
use doze_module_proto::msg::value::{Node, PathSeg, Value};
use doze_module_proto::peer::{self, CallOpts, Handler, Incoming, Peer};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

fn s(v: &str) -> Node {
    Node::bare(Value::String(v.into()))
}
fn n(v: u32) -> Node {
    Node::bare(Value::Number(v.to_string()))
}
fn map(entries: &[(&str, Node)]) -> Node {
    Node::bare(Value::Map(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    ))
}
fn list(items: Vec<Node>) -> Node {
    Node::bare(Value::List(items))
}

fn instance(version: &str, spec: Node, data: &str) -> Instance {
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

fn good() -> Node {
    let ch = |name: &str, size: u32| map(&[("name", s(name)), ("size", n(size))]);
    map(&[
        ("greeting", s("hi")),
        ("channel", list(vec![ch("a", 4), ch("b", 2)])),
    ])
}

/// The host's side of `fetch.*`, counting calls.
fn host(majors: Arc<AtomicU32>) -> Handler {
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

async fn start(majors: Arc<AtomicU32>) -> (Peer, Hello, tokio::process::Child) {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_doze-module-echo"));
    cmd.kill_on_drop(true);
    let (child, ch) = doze_os::channel::spawn_module(cmd).await.unwrap();
    let (peer, hello) = peer::accept_module(ch, host(majors), None).await.unwrap();
    (peer, hello, child)
}

async fn decode(p: &Peer, inst: &Instance) -> Result<Unit, Error> {
    p.call(Method::Decode, inst, CallOpts::default()).await
}

fn has(e: &Error, path: &[PathSeg], text: &str) -> bool {
    e.diagnostics
        .iter()
        .any(|d| d.path == path && d.message.contains(text))
}

#[tokio::test(flavor = "current_thread")]
async fn the_manifest_comes_from_the_derive_and_the_registrations() {
    let (_p, hello, _c) = start(Arc::default()).await;
    let m = hello.manifest;
    let names: Vec<_> = m.schema.attrs.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["greeting", "buffer", "settings"]);
    let buffer = &m.schema.attrs[1];
    assert_eq!(
        (buffer.required, buffer.default.clone()),
        (false, Some(Value::Number("10".into())))
    );
    assert_eq!(m.schema.attrs[0].since.as_deref(), Some("2"));
    let ch = &m.schema.blocks[0];
    assert_eq!(
        (
            ch.name.as_str(),
            ch.labels.clone(),
            ch.schema.attrs[0].name.as_str()
        ),
        ("channel", vec!["name".to_string()], "size")
    );
    assert_eq!(
        m.caps,
        caps::OBJECTS,
        "capabilities must be exactly what was registered"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn decode_reports_every_problem_at_its_path() {
    let (p, _h, _c) = start(Arc::default()).await;
    decode(&p, &instance("2", good(), "")).await.unwrap();
    let ch = |name: &str, size: u32| map(&[("name", s(name)), ("size", n(size))]);
    let bad = map(&[
        ("bufer", n(3)),
        ("greeting", s("hi")),
        ("channel", list(vec![ch("a", 4), ch("b", 0), ch("a", 1)])),
    ]);
    let e = decode(&p, &instance("1", bad, "")).await.unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Invalid);
    let key = |k: &str| PathSeg::Key(k.into());
    assert!(
        has(&e, &[key("bufer")], "unknown argument `bufer`"),
        "{:#?}",
        e.diagnostics
    );
    assert!(
        e.diagnostics
            .iter()
            .any(|d| d.hint.as_deref() == Some("did you mean `buffer`?"))
    );
    assert!(
        has(&e, &[key("greeting")], "needs engine version 2"),
        "{:#?}",
        e.diagnostics
    );
    assert!(
        has(
            &e,
            &[key("channel"), PathSeg::Index(2)],
            "declared more than once"
        ),
        "{:#?}",
        e.diagnostics
    );
    // The module's own check runs only once the spec decodes.
    let zero = map(&[("channel", list(vec![ch("a", 4), ch("b", 0)]))]);
    let e = decode(&p, &instance("2", zero, "")).await.unwrap_err();
    assert!(
        has(
            &e,
            &[key("channel"), PathSeg::Index(1), key("size")],
            "at least one"
        ),
        "{:#?}",
        e.diagnostics
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_required_calls_and_a_fetch_callback() {
    let majors = Arc::new(AtomicU32::new(0));
    let (p, _h, _c) = start(majors.clone()).await;
    let dir = std::env::temp_dir().join(format!("doze-module-echo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let inst = instance("2", good(), &dir.to_string_lossy());
    let net: Vec<Listener> = p
        .call(Method::Network, &inst, CallOpts::default())
        .await
        .unwrap();
    assert_eq!(net[0].name, "main");
    let c: Connection = p
        .call(Method::Connection, &inst, CallOpts::default())
        .await
        .unwrap();
    assert_eq!(
        (c.env_var.as_str(), c.url.as_str()),
        ("ECHO_URL", "echo:///e")
    );
    let args = ResolveArgs {
        instance: inst.clone(),
        target: "t".into(),
        pins: vec![],
    };
    let r: Resolved = p
        .call(Method::Resolve, &args, CallOpts::default())
        .await
        .unwrap();
    assert_eq!(
        (r.toolchain.full.as_str(), r.pins[0].hashes["t"].as_str()),
        ("2.4.1", "sha256:abc")
    );
    // With a pin the major is not asked again.
    let pinned = ResolveArgs {
        pins: vec![Pin {
            engine: "echo".into(),
            spec: "2".into(),
            resolved: "2.4.1".into(),
            hashes: BTreeMap::new(),
        }],
        ..args
    };
    let _: Resolved = p
        .call(Method::Resolve, &pinned, CallOpts::default())
        .await
        .unwrap();
    assert_eq!(majors.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn provision_plan_and_objects() {
    let (p, _h, _c) = start(Arc::default()).await;
    let dir = std::env::temp_dir().join(format!("doze-module-echo-pp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let inst = instance("2", good(), &dir.to_string_lossy());
    let toolchain = Toolchain {
        engine: "echo".into(),
        full: "2.4.1".into(),
        bin_dir: "/cache/echo/bin".into(),
        tools: BTreeMap::new(),
    };
    let wt = WithToolchain {
        instance: inst.clone(),
        toolchain,
    };
    let first: Provisioned = p
        .call(Method::Provision, &wt, CallOpts::default())
        .await
        .unwrap();
    let again: Provisioned = p
        .call(Method::Provision, &wt, CallOpts::default())
        .await
        .unwrap();
    assert_eq!((first, again), (Provisioned::Fresh, Provisioned::Existing));
    let plan: Plan = p
        .call(Method::Plan, &wt, CallOpts::default())
        .await
        .unwrap();
    assert!(plan.processes[0].program.ends_with("echo-engine"));
    assert!(matches!(&plan.processes[0].ready, Some(Ready::Socket(s)) if s.ends_with("echo.sock")));
    let objs: Vec<Object> = p
        .call(Method::Objects, &inst, CallOpts::default())
        .await
        .unwrap();
    assert_eq!(
        objs.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn a_capability_not_registered_is_not_offered() {
    let (p, _h, _c) = start(Arc::default()).await;
    let wt = WithToolchain {
        instance: instance("2", good(), ""),
        toolchain: Toolchain {
            engine: "echo".into(),
            full: "2".into(),
            bin_dir: String::new(),
            tools: BTreeMap::new(),
        },
    };
    let e = p
        .call::<_, Unit>(Method::Converge, &wt, CallOpts::default())
        .await
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Unsupported);
}
