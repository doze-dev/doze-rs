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

pub fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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
