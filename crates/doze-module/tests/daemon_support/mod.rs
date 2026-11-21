//! The echo module's stand-ins for the daemon's tests: a finder that finds only echo, downloads that hand out its
//! engine, and a project with one echo instance (`e`, channel `a`), a disabled one and a task.
#![allow(dead_code, unreachable_pub)] // each test file uses part of it

use doze_daemon::catalog::Finder;
use doze_module_host::{BoxFuture, Downloads};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::run::{Ensure, Ensured};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Finds the echo module's build, and says which types it was asked for.
#[derive(Default)]
pub struct Find {
    pub asked: Mutex<Vec<(String, Vec<String>)>>,
    /// A build that serves engine 1 only, found for major 1 (else the echo serves 1 and 2).
    pub only1: Option<PathBuf>,
    /// Another build of echo found for every major (a copy named `…-supervised`).
    pub echo: Option<PathBuf>,
}

impl Finder for Find {
    fn find<'a>(
        &'a self,
        ty: &'a str,
        majors: &'a [String],
        _lock: Option<&'a Path>,
    ) -> BoxFuture<'a, Result<PathBuf, String>> {
        self.asked
            .lock()
            .unwrap()
            .push((ty.to_string(), majors.to_vec()));
        let only1 = self
            .only1
            .clone()
            .filter(|_| majors == ["1"])
            .or_else(|| self.echo.clone());
        let found = (ty == "echo")
            .then(|| only1.unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo"))))
            .ok_or_else(|| {
                if ty == "offline" {
                    format!("fetching module index doze/{ty}/index.yaml: registry.test: no answer within 3s")
                } else {
                    format!("module doze/{ty} is not published in the registry")
                }
            });
        Box::pin(async move { found })
    }
}

/// Hands out the echo engine's folder and remembers the hash each `ensure` expected.
#[derive(Default)]
pub struct Fetch {
    pub expected: Mutex<Vec<Option<String>>>,
    pub majors: std::sync::atomic::AtomicUsize,
}

pub struct Shared(pub Arc<Fetch>);

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

/// A loopback port nobody holds, below every system's ephemeral range (Linux starts at 32768, macOS and Windows at
/// 49152), and never the same one twice in this process: a port from `bind(":0")` can be handed straight back to a
/// test running alongside (Windows does), as doze-module-test's `free_port` found on Linux.
pub fn free_port() -> u16 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let start = std::process::id().wrapping_mul(97) % 12_000;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let port = u16::try_from(20_000 + (start + n) % 12_000).unwrap();
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

pub fn project(tag: &str) -> (PathBuf, u16) {
    let d = std::env::temp_dir().join(format!("doze-daemon-catalog-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("home")).unwrap();
    let port = free_port();
    let off = free_port();
    // An HCL string reads `\U` as an escape: a Windows path goes in with forward slashes, which Windows accepts.
    let calls = d.join("calls").to_string_lossy().replace('\\', "/");
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

/// A raw protocol-2 client: lines out, frames in.
pub struct Raw {
    w: doze_os::local::LocalStream,
    r: std::io::BufReader<doze_os::local::LocalStream>,
}

impl Raw {
    pub fn open(sock: &Path) -> Raw {
        let w = doze_os::local::LocalStream::connect(sock).unwrap();
        w.set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        let r = std::io::BufReader::new(w.try_clone().unwrap());
        let mut c = Raw { w, r };
        c.say(r#"{"hello":{"proto":2}}"#);
        c.hear();
        c
    }
    pub fn say(&mut self, line: &str) {
        use std::io::Write;
        self.w.write_all(format!("{line}\n").as_bytes()).unwrap();
    }
    pub fn hear(&mut self) -> serde_json::Value {
        use std::io::BufRead;
        let mut s = String::new();
        self.r.read_line(&mut s).unwrap();
        serde_json::from_str(&s).unwrap()
    }
    /// A unary answer (the frame's `response`).
    pub fn ask(&mut self, line: &str) -> serde_json::Value {
        self.say(line);
        self.hear().get("response").cloned().unwrap_or_default()
    }
}

/// The project in `dir` served as `doze __daemon` serves it (the catalog with the echo stand-ins, its fronts, and
/// what adds instances at run time), on a socket in `dir`.
/// `None` where the system has no local control socket yet (Windows).
pub async fn serve(
    dir: &Path,
) -> Option<(
    doze_daemon::Daemon,
    doze_daemon::Serving,
    PathBuf,
    Arc<doze_daemon::front::Front>,
)> {
    serve_at(dir, dir.join("ctl.sock")).await
}

/// [`serve`], its control socket at `sock` (where the CLI looks for the stack's daemon).
pub async fn serve_at(
    dir: &Path,
    sock: PathBuf,
) -> Option<(
    doze_daemon::Daemon,
    doze_daemon::Serving,
    PathBuf,
    Arc<doze_daemon::front::Front>,
)> {
    use doze_daemon::catalog::Catalog;
    let lock = dir.join("doze.lock");
    let downloads = Arc::new(doze_engine::modules::Pinned {
        inner: Shared(Arc::new(Fetch::default())),
        pins: doze_daemon::catalog::lock_pins(&lock),
    });
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find::default()),
        downloads,
        lock,
        dir.join("home"),
    ));
    let env = std::collections::BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let config = dir.join("doze.hcl");
    let mut a = doze_daemon::entry::assemble(&config, "0.0.0", cat.clone(), env.clone())
        .await
        .unwrap();
    let front = Arc::new(a.open_front());
    cat.attach(&a.engine);
    let tls = a.tls.clone();
    let live = doze_daemon::live::Live::new(cat, &config, "0.0.0", env, front.clone());
    let d = doze_daemon::Daemon::at(
        a.engine,
        a.labels,
        doze_daemon::StackInfo::default(),
        sock.clone(),
    )
    .with_modules(a.modules)
    .with_live(live);
    d.answer_front(&front, tls);
    let s = d.serve();
    if let Err(e) = &s
        && e.kind() == std::io::ErrorKind::Unsupported
    {
        eprintln!("skipped: {e}");
        front.close();
        d.engine().shutdown().await;
        return None;
    }
    let s = s.expect("serving");
    Some((d, s, sock, front))
}
