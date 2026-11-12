//! A host and a module peer over an in-process channel, with a manifest that declares what each test needs.
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used)]

use doze_module_proto::msg::hello::{Hello, Manifest, Schema, WireVersion, caps};
use doze_module_proto::peer::{self, Handler, Incoming, Peer};
use std::future::Future;
use std::sync::Arc;

pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Incoming) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |c| Box::pin(f(c)))
}

pub fn nothing() -> Handler {
    handler(|_c: Incoming| async {})
}

pub fn manifest(caps: u64) -> Manifest {
    Manifest {
        engine: "echo".into(),
        category: "test".into(),
        title: "Echo".into(),
        tagline: "a test module".into(),
        versions: vec![],
        exact_dots: 2,
        default_port: 7000,
        boot_budget_ms: None,
        supervised: false,
        backend_socket: Some("echo.sock".into()),
        caps,
        schema: Schema::default(),
        example: "echo \"e\" {}".into(),
        console: None,
        actions: vec![],
        targets: vec![],
    }
}

pub fn hello(caps: u64, minor: u16) -> Hello {
    Hello {
        version: WireVersion { major: 2, minor },
        module: "echo".into(),
        release: "0.0.0".into(),
        manifest: manifest(caps),
    }
}

/// Host and module connected; the host serves `host`, the module serves `module`.
pub async fn connected(module_caps: u64, host: Handler, module: Handler) -> (Peer, Peer) {
    let (a, b) = doze_os::channel::pair().await.unwrap();
    let h = tokio::spawn(peer::accept_module(a, host, None));
    let (m, _welcome) = peer::connect_host(b, hello(module_caps, 0), module)
        .await
        .unwrap();
    let (h, _hello) = h.await.unwrap().unwrap();
    (h, m)
}

pub const ALL: u64 = caps::CONSOLE | caps::WIRE | caps::ADMIN;

/// A minimal instance for calls that need one.
pub fn instance() -> doze_module_proto::msg::instance::Instance {
    use doze_module_proto::msg::value::{Node, Value};
    doze_module_proto::msg::instance::Instance {
        name: "e".into(),
        engine: "echo".into(),
        version: String::new(),
        data_dir: String::new(),
        socket_dir: String::new(),
        spec: Node::bare(Value::Map(Default::default())),
        host: String::new(),
        listeners: Default::default(),
        deps: vec![],
        toolchain: None,
    }
}
