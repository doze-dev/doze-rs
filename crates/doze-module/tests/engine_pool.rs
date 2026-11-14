//! The engine runs an instance its module plans, through the module pool (core PLAN.md E2): the echo module starts on
//! first use, resolves with the lock's pins, plans its engine, the engine runs it, and `sync` records its objects. The
//! lock's pin wins over the hash the module asks `fetch.ensure` for.

mod echo_support;

use doze_engine::modules::{ModuleInstance, Pinned};
use doze_engine::{Cause, Engine, Service};
use doze_module_host::{BoxFuture, Downloads, Pool, Services};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::run::{Ensure, Ensured, Pin};
use echo_support::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
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

fn pin(hash: &str) -> Pin {
    Pin {
        engine: "echo".into(),
        spec: "2".into(),
        resolved: "2.4.1".into(),
        hashes: BTreeMap::from([("t1".to_string(), hash.to_string())]),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_engine_runs_a_module_instance_and_the_lock_pin_wins() {
    let dir = tempfile_dir();
    let fetch = Arc::new(Fetch::default());
    let services = Services {
        downloads: Arc::new(Pinned {
            inner: Shared(fetch.clone()),
            pins: vec![pin("sha256:lock")],
        }),
        backends: None,
        notices: None,
    };
    let locate: doze_module_host::Locate =
        Arc::new(|_| Some(PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo"))));
    let pool = Arc::new(Pool::new(services, locate, Duration::from_secs(60)));
    // The module is handed a stale pin for the same build: the lock's hash must be what the download checks.
    let base = instance("2", good(), "");
    let m = ModuleInstance::new(pool.clone(), base, vec![pin("sha256:module")], "t1", true);
    let e = Engine::builder("t", &dir)
        .service(Service::planned("e", Arc::new(m)))
        .build()
        .unwrap();
    let ep = e.wake("e", Cause::Command).await.unwrap();
    assert!(ep.backend.starts_with("unix:"), "{ep:?}");
    assert_eq!(
        fetch.expected.lock().unwrap().clone(),
        [Some("sha256:lock".to_string())]
    );
    let synced = e.sync(None).await.unwrap();
    let names: Vec<String> = synced
        .iter()
        .flat_map(|(_, ch)| ch.iter().map(|(_, o)| o.name.clone()))
        .collect();
    assert_eq!(names, ["a", "b"]);
    e.shutdown().await;
    assert_eq!(pool.running().await, ["echo"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[allow(clippy::unwrap_used)] // test code
fn tempfile_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-engine-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
