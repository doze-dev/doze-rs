//! The engine runs an instance its module plans, through the module pool (core PLAN.md E2): the echo module starts on
//! first use, resolves with the lock's pins, plans its engine, the engine runs it, and `sync` records its objects. The
//! lock's pin wins over the hash the module asks `fetch.ensure` for.
#![allow(clippy::unwrap_used)]

mod echo_support;

use doze_engine::applied::Change;
use doze_engine::modules::{ModuleInstance, Pinned};
use doze_engine::{Cause, Engine, Service};
use doze_module_host::{BoxFuture, Downloads, Pool, Services};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::run::{Ensure, Ensured, Pin};
use doze_module_proto::msg::value::Node;
use echo_support::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The download side: hands out the echo engine's folder and remembers the hash each `ensure` expected.
#[derive(Default)]
struct Fetch {
    expected: Mutex<Vec<Option<String>>>,
}

/// The fetch side the pool owns, sharing what it saw with the test.
struct Shared(Arc<Fetch>);

impl Downloads for Shared {
    fn resolve_major<'a>(
        &'a self,
        _e: &'a str,
        _m: &'a str,
    ) -> BoxFuture<'a, Result<String, Error>> {
        Box::pin(async { Ok("2.4.1".to_string()) })
    }

    fn ensure<'a>(&'a self, req: &'a Ensure) -> BoxFuture<'a, Result<Ensured, Error>> {
        if let Ok(mut e) = self.0.expected.lock() {
            e.push(req.expected_sha.clone());
        }
        let engine = PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo-engine"));
        let bin_dir = engine
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Box::pin(async move {
            Ok(Ensured {
                bin_dir,
                digest: "sha256:lock".into(),
            })
        })
    }
}

fn pin_for(resolved: &str, target: &str, hash: &str) -> Pin {
    Pin {
        engine: "echo".into(),
        spec: "2".into(),
        resolved: resolved.into(),
        hashes: BTreeMap::from([(target.to_string(), hash.to_string())]),
    }
}

fn pin(hash: &str) -> Pin {
    pin_for("2.4.1", "t1", hash)
}

/// The lock's pins, the one that applies last: one for another build of the engine, one for this build on another
/// target. Matching on the engine alone, or taking a pin's first hash whatever its target, picks a decoy.
fn lock_pins() -> Vec<Pin> {
    vec![
        pin_for("2.4.0", "t1", "sha256:other-build"),
        pin_for("2.4.1", "t2", "sha256:other-target"),
        pin("sha256:lock"),
    ]
}

/// The echo spec with these channels, recording the order of the module's calls in `calls`.
fn spec(channels: &[&str], calls: &Path) -> Node {
    let ch = |name: &str| map(&[("name", s(name)), ("size", n(1))]);
    map(&[
        ("greeting", s("hi")),
        ("settings", map(&[("calls", s(&calls.to_string_lossy()))])),
        ("channel", list(channels.iter().map(|c| ch(c)).collect())),
    ])
}

fn engine(dir: &Path, pool: &Arc<Pool>, spec: Node) -> Engine {
    // The module is handed a stale pin for the same build: the lock's hash must be what the download checks.
    let base = instance("2", spec, "");
    let m = ModuleInstance::new(pool.clone(), base, vec![pin("sha256:module")], "t1", true);
    Engine::builder("t", dir)
        .service(Service::planned("e", Arc::new(m)))
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_engine_runs_a_module_instance_and_the_lock_pin_wins() {
    let dir = tempfile_dir();
    let calls = dir.join("calls");
    let fetch = Arc::new(Fetch::default());
    let services = Services {
        downloads: Arc::new(Pinned {
            inner: Shared(fetch.clone()),
            pins: lock_pins(),
        }),
        backends: None,
        notices: None,
    };
    let locate: doze_module_host::Locate =
        Arc::new(|_| Some(PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo"))));
    let pool = Arc::new(Pool::new(services, locate, Duration::from_secs(60)));
    let e = engine(&dir, &pool, spec(&["a", "b"], &calls));
    let ep = e.wake("e", Cause::Command).await.unwrap();
    assert!(ep.backend.starts_with("unix:"), "{ep:?}");
    assert_eq!(
        fetch.expected.lock().unwrap().clone(),
        [Some("sha256:lock".to_string())]
    );
    // Each call checks the spec first; `decode` is the call that does nothing after its check. So: decode, resolve,
    // provision, plan.
    let order = std::fs::read_to_string(&calls).unwrap();
    let order: Vec<&str> = order.lines().collect();
    assert_eq!(
        order,
        [
            "check",
            "check",
            "resolve",
            "check",
            "provision",
            "check",
            "plan"
        ]
    );
    let synced = e.sync(None).await.unwrap();
    let names: Vec<String> = synced
        .iter()
        .flat_map(|(_, ch)| ch.iter().map(|(_, o)| o.name.clone()))
        .collect();
    assert_eq!(names, ["a", "b"]);
    e.shutdown().await;
    // `b` is no longer declared. The echo module does not offer `prune` (tests/echo.rs pins its capabilities), so the
    // sync drops `b` from the record without calling it; calling it would fail the sync.
    let e = engine(&dir, &pool, spec(&["a"], &calls));
    let synced = e.sync(None).await.unwrap();
    let changes: Vec<(Change, String)> = synced
        .iter()
        .flat_map(|(_, ch)| ch.iter().map(|(c, o)| (*c, o.name.clone())))
        .collect();
    assert_eq!(changes, [(Change::Delete, "b".to_string())]);
    e.shutdown().await;
    assert_eq!(pool.running().await, ["echo"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The pin for the build and the target asked for, and no other.
#[test]
fn a_pin_is_for_one_build_on_one_target() {
    let p = Pinned {
        inner: (),
        pins: lock_pins(),
    };
    assert_eq!(p.pinned("echo", "2.4.1", "t1"), Some("sha256:lock"));
    assert_eq!(p.pinned("echo", "2.4.0", "t1"), Some("sha256:other-build"));
    assert_eq!(p.pinned("echo", "2.4.1", "t2"), Some("sha256:other-target"));
    assert_eq!(p.pinned("echo", "2.4.1", "t3"), None);
    assert_eq!(p.pinned("echo", "2.5.0", "t1"), None);
    assert_eq!(p.pinned("other", "2.4.1", "t1"), None);
}

fn tempfile_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-engine-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
