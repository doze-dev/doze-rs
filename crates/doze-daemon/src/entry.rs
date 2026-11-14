//! `doze __daemon --config <path>` (Go's `runDaemon`): the stack's doze.hcl loaded through doze-config, made the
//! engine's stack, and served over control protocol 2 on its socket until SIGTERM or SIGINT; then the engine stops
//! everything it started and the socket and pid file go.
//!
//! Interim (D1, next step): module blocks are accepted with what every module has and their instances say, when woken,
//! that this daemon does not run modules yet. The module catalog over the pool (each module's own manifest and
//! attributes, F1's downloads and doze.lock's pins) replaces `Pending` and `NoModule` without changing this file's
//! shape. Apps, services and tasks run in full.

use crate::ctl::{self, RunDir};
use crate::{Daemon, Labels, StackInfo};
use doze_config::modules::{ModuleInfo, Modules};
use doze_config::{Instance, Kind, Spec};
use doze_engine::stack::{BoxFuture, PlanCtx, Planned, Planner};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

/// Runs the daemon for `args` (what follows `__daemon`); its exit code.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
pub fn main(args: &[String], version: &str) -> ExitCode {
    let config = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map_or_else(|| PathBuf::from("doze.hcl"), PathBuf::from);
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("doze daemon: runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(&config, version)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("doze daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
async fn run(config: &Path, version: &str) -> Result<(), String> {
    let (path, version) = (config.to_string_lossy().into_owned(), version.to_owned());
    let cfg = tokio::task::spawn_blocking(move || load(&path, &version))
        .await
        .map_err(|e| e.to_string())??;
    let run = RunDir::of(Path::new(&cfg.data_dir));
    doze_os::fs::private_dir(run.path()).map_err(|e| e.to_string())?;
    if ctl::running(&run) {
        return Err(format!(
            "a daemon already serves {}",
            run.socket().display()
        ));
    }
    let planner = |i: &Instance| -> Arc<dyn Planner> { Arc::new(NoModule { ty: i.ty.clone() }) };
    let wiring = doze_config::stack::Wiring {
        planner: &planner,
        extra_env: ca_env(Path::new(&cfg.home)),
    };
    let engine = doze_engine::Engine::new(doze_config::stack::to_stack(&cfg, &wiring))
        .map_err(|e| e.to_string())?;
    let info = StackInfo {
        listen: cfg.listen.clone(),
        dir: cfg.dir.clone(),
        loaded: Some(std::time::SystemTime::now()),
    };
    let daemon = Daemon::new(engine.clone(), labels(&cfg), info);
    let serving = daemon
        .serve()
        .map_err(|e| format!("serving {}: {e}", run.socket().display()))?;
    ctl::write_pid(&run).map_err(|e| format!("writing the pid file: {e}"))?;
    eprintln!(
        "doze daemon: serving {} on {}",
        cfg.stack(),
        run.socket().display()
    );
    let asked = doze_os::signal::stop_requested().await;
    eprintln!(
        "doze daemon: stopping ({})",
        asked.map_or_else(|e| e.to_string(), |()| "asked to".into())
    );
    serving.stop();
    engine.shutdown().await;
    let _ = std::fs::remove_file(run.pid_file());
    Ok(())
}

/// The stack at `path`, as the CLI loads it: this process's environment (`DOZE_VAR_*` included), doze-names' checks.
fn load(path: &str, version: &str) -> Result<doze_config::Config, String> {
    let domains = Domains;
    let host = doze_config::host::OsHost { domains: &domains };
    let opts = doze_config::Options {
        host: &host,
        modules: &Pending,
        vars: BTreeMap::new(),
        version: version.into(),
    };
    doze_config::load(path, &opts).map_err(|e| e.to_string())
}

/// doze's CA for what it runs (Go's `caEnv`), when doze has made one.
fn ca_env(home: &Path) -> BTreeMap<String, String> {
    let tls = home.join("tls");
    if !tls.join("ca.pem").exists() {
        return BTreeMap::new();
    }
    doze_config::stack::ca_env(&tls.join("ca.pem"), &tls.join("bundle.pem"))
}

/// What the config says about each instance, for the views.
fn labels(cfg: &doze_config::Config) -> BTreeMap<String, Labels> {
    cfg.instances
        .iter()
        .map(|i| {
            let run = match &i.spec {
                Spec::Command(c) => c.run.clone(),
                _ => String::new(),
            };
            let l = Labels {
                kind: match i.kind() {
                    Kind::Module => String::new(),
                    k => k.as_str().into(),
                },
                engine: i.ty.clone(),
                version: i.version.clone(),
                run,
                needs: i.deps.iter().map(|d| d.name.clone()).collect(),
                endpoint: doze_config::instance_addr(i, &Pending).unwrap_or_default(),
                domain: i.domains.first().cloned().unwrap_or_default(),
                paused: !i.enabled,
                ..Labels::default()
            };
            (i.name.clone(), l)
        })
        .collect()
}

/// doze-names' domain checks for the config.
struct Domains;

impl doze_config::host::DomainChecks for Domains {
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        doze_names::domains::valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        doze_names::domains::route_domain(host)
    }
    fn check_certificate(&self, cert: &Path, key: &Path, _domain: &str) -> Result<(), String> {
        for f in [cert, key] {
            if !f.exists() {
                return Err(format!("{} does not exist", f.display()));
            }
        }
        Ok(())
    }
}

/// The interim module catalog: every module type is accepted with what every module has (see the module docs).
struct Pending;

impl Modules for Pending {
    fn lookup(&self, ty: &str, _version: &str) -> Option<ModuleInfo> {
        self.cached(ty)
    }
    fn cached(&self, _ty: &str) -> Option<ModuleInfo> {
        Some(ModuleInfo::default())
    }
    fn names(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A module instance this daemon cannot run yet.
#[derive(Debug)]
struct NoModule {
    ty: String,
}

impl Planner for NoModule {
    fn plan<'a>(&'a self, _ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>> {
        let ty = self.ty.clone();
        Box::pin(async move {
            Err(format!(
                "module {ty:?}: the Rust daemon does not run modules yet (the module catalog over the pool comes next in D1)"
            ))
        })
    }
}
