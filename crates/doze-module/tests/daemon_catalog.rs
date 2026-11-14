//! The daemon's module catalog (core PLAN.md D1) end to end with the echo module: a doze.hcl module block is loaded
//! through what the module declares, its block reaches the module as the protocol's value tree, what it publishes is
//! what the config's references and the views read, the lock's engine pin wins over the module's hash, and a
//! connection to its declared port wakes it through the daemon's front.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use doze_daemon::catalog::{Catalog, Finder};
use doze_daemon::front::Front;
use doze_engine::modules::Pinned;
use doze_module_host::{BoxFuture, Downloads};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::run::{Ensure, Ensured};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Finds the echo module's build, and says which types it was asked for.
#[derive(Default)]
struct Find {
    asked: Mutex<Vec<(String, Vec<String>)>>,
}

impl Finder for Find {
    fn find<'a>(
        &'a self,
        ty: &'a str,
        majors: &'a [String],
        _lock: &'a Path,
    ) -> BoxFuture<'a, Result<PathBuf, String>> {
        self.asked
            .lock()
            .unwrap()
            .push((ty.to_string(), majors.to_vec()));
        let found = (ty == "echo")
            .then(|| PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo")))
            .ok_or_else(|| format!("module doze/{ty} is not published in the registry"));
        Box::pin(async move { found })
    }
}

/// Hands out the echo engine's folder and remembers the hash each `ensure` expected.
#[derive(Default)]
struct Fetch {
    expected: Mutex<Vec<Option<String>>>,
    majors: std::sync::atomic::AtomicUsize,
}

struct Shared(Arc<Fetch>);

impl Downloads for Shared {
    fn resolve_major<'a>(
        &'a self,
        _e: &'a str,
        _m: &'a str,
    ) -> BoxFuture<'a, Result<String, Error>> {
        self.0
            .majors
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok("2.4.1".to_string()) })
    }

    fn ensure<'a>(&'a self, req: &'a Ensure) -> BoxFuture<'a, Result<Ensured, Error>> {
        self.0
            .expected
            .lock()
            .unwrap()
            .push(req.expected_sha.clone());
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

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn project(tag: &str) -> (PathBuf, u16) {
    let d = std::env::temp_dir().join(format!("doze-daemon-catalog-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("home")).unwrap();
    let port = free_port();
    let off = free_port();
    let calls = d.join("calls").to_string_lossy().into_owned();
    let hcl = format!(
        "module echo \"e\" {{\n  version = \"2\"\n  port = {port}\n  greeting = \"hi\"\n  settings = {{\n    calls = \"{calls}\"\n  }}\n  channel \"a\" {{\n    size = 2\n  }}\n}}\n\nmodule echo \"off\" {{\n  version = \"2\"\n  port = {off}\n  enabled = false\n}}\n\ntask \"show\" {{\n  run = \"true\"\n  manual = true\n  env = {{\n    U = module.e.url\n    T = module.e.env.ECHO_URL\n  }}\n}}\n"
    );
    std::fs::write(d.join("doze.hcl"), hcl).unwrap();
    let lock = format!(
        "engines:\n  echo:\n    \"2\":\n      resolved: 2.4.1\n      source: mirror\n      hashes:\n        {}: sha256:pinned\n",
        doze_fetch::triple()
    );
    std::fs::write(d.join("doze.lock"), lock).unwrap();
    (d, port)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_module_block_runs_through_the_catalog_and_a_connection_wakes_it() {
    let (dir, port) = project("wake");
    let lock = dir.join("doze.lock");
    let fetch = Arc::new(Fetch::default());
    let find = Arc::new(Find::default());
    let downloads = Arc::new(Pinned {
        inner: Shared(fetch.clone()),
        pins: doze_daemon::catalog::lock_pins(&lock),
    });
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        find.clone(),
        downloads,
        lock,
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let a = doze_daemon::entry::assemble(&dir.join("doze.hcl"), "0.0.0", cat.clone(), env)
        .await
        .unwrap();
    assert_eq!(
        *find.asked.lock().unwrap(),
        [("echo".to_string(), vec!["2".to_string()])]
    );

    let addr = format!("127.0.0.1:{port}");
    published_as_the_module_says(&a, &addr);
    wake_through_the_front(&a, &addr, &dir, &fetch).await;
    a.engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the module publishes: the views' variables, the config's references.
fn published_as_the_module_says(a: &doze_daemon::entry::Assembled, addr: &str) {
    let l = a.labels.get("e").unwrap();
    assert_eq!(
        (l.env_var.as_str(), l.url.as_str()),
        ("ECHO_URL", format!("echo://{addr}/e").as_str())
    );
    assert_eq!(
        l.env,
        BTreeMap::from([
            ("ECHO_TOKEN".to_string(), "doze".to_string()),
            ("ECHO_URL".to_string(), format!("echo://{addr}/e")),
        ])
    );
    let show = a.cfg.instances.iter().find(|i| i.name == "show").unwrap();
    let c = match &show.spec {
        doze_config::Spec::Command(c) => Some(c),
        _ => None,
    }
    .expect("a task");
    assert_eq!(c.env.get("U"), Some(&format!("echo://{addr}/e")));
    assert_eq!(c.env.get("T"), Some(&format!("echo://{addr}/e")));
}

/// A connection wakes it, and the lock's pin won; the block's arguments and labelled blocks reached the module.
async fn wake_through_the_front(
    a: &doze_daemon::entry::Assembled,
    addr: &str,
    dir: &Path,
    fetch: &Fetch,
) {
    let fronted: Vec<String> = a.fronted().into_iter().map(|f| f.name).collect();
    assert_eq!(fronted, ["e"], "a disabled module gets no front");
    let front = Front::open(&a.engine, &a.fronted()).await;
    let conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (r, mut w) = conn.into_split();
    w.write_all(b"ping\n").await.unwrap();
    let mut lines = BufReader::new(r).lines();
    let mut seen = Vec::new();
    while let Ok(Ok(Some(line))) =
        tokio::time::timeout(Duration::from_secs(20), lines.next_line()).await
    {
        seen.push(line.clone());
        if line.contains("ping") {
            break;
        }
    }
    assert!(
        seen.iter().any(|l| l.contains("ping")),
        "the echo answered through the front: {seen:?}"
    );
    let e = a.engine.get("e").unwrap();
    assert!(e.state.running() && e.conns == 1, "{e:?}");
    assert_eq!(
        *fetch.expected.lock().unwrap(),
        [Some("sha256:pinned".to_string())]
    );
    // The block's own arguments reached the module (its `settings` map says where it records its calls), its
    // labelled blocks too (the channel is an object, by its label).
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
    assert!(calls.lines().any(|l| l == "plan"), "{calls}");
    let synced = a.engine.sync(None).await.unwrap();
    let objects: Vec<String> = synced
        .iter()
        .flat_map(|(_, ch)| ch.iter().map(|(_, o)| o.name.clone()))
        .collect();
    assert_eq!(objects, ["a"]);
    drop(w);
    drop(lines);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        a.engine.get("e").map(|e| e.conns),
        Some(0),
        "a closed connection is released"
    );
    front.close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_module_that_cannot_be_found_says_why_at_load() {
    let (dir, _) = project("missing");
    std::fs::write(
        dir.join("doze.hcl"),
        "module nosuch \"x\" {\n  port = 7999\n}\n",
    )
    .unwrap();
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find::default()),
        Arc::new(Shared(Arc::default())),
        dir.join("doze.lock"),
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let err = doze_daemon::entry::assemble(&dir.join("doze.hcl"), "0.0.0", cat, env)
        .await
        .unwrap_err();
    assert!(err.contains("not published in the registry"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_version_the_module_does_not_serve_is_refused_at_load() {
    let (dir, port) = project("version");
    std::fs::write(
        dir.join("doze.hcl"),
        format!("module echo \"e\" {{\n  version = \"7\"\n  port = {port}\n}}\n"),
    )
    .unwrap();
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find::default()),
        Arc::new(Shared(Arc::default())),
        dir.join("doze.lock"),
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let err = doze_daemon::entry::assemble(&dir.join("doze.hcl"), "0.0.0", cat, env)
        .await
        .unwrap_err();
    assert!(err.contains("serves 1, 2"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The engine pin a module reports at its first start is written to doze.lock (Go's `up` wrote it) and kept for the
/// instance's next start, which asks no mirror.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_engine_pin_a_module_reports_is_written_to_the_lock() {
    let (dir, port) = project("pins");
    let lock = dir.join("doze.lock");
    std::fs::remove_file(&lock).unwrap();
    let fetch = Arc::new(Fetch::default());
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find::default()),
        Arc::new(Shared(fetch.clone())),
        lock.clone(),
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let a = doze_daemon::entry::assemble(&dir.join("doze.hcl"), "0.0.0", cat, env)
        .await
        .unwrap();
    a.engine
        .wake("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    let pins = doze_daemon::catalog::lock_pins(&lock);
    let echo: Vec<_> = pins.iter().filter(|p| p.engine == "echo").collect();
    assert_eq!(echo.len(), 1, "{pins:?}");
    assert_eq!(
        (echo[0].spec.as_str(), echo[0].resolved.as_str()),
        ("2", "2.4.1")
    );
    assert_eq!(
        echo[0]
            .hashes
            .get(&doze_fetch::triple())
            .map(String::as_str),
        Some("sha256:lock")
    );
    // Asleep and woken again: the pin it reported is used, the mirror is not asked a second time.
    a.engine
        .stop("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    a.engine
        .wake("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    assert_eq!(fetch.majors.load(std::sync::atomic::Ordering::SeqCst), 1);
    let _ = port;
    a.engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}
