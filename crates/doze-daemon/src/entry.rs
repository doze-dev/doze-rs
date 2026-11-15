//! `doze __daemon --config <path>` (Go's `runDaemon`): the stack's doze.hcl loaded through doze-config, made the
//! engine's stack, and served over control protocol 2 on its socket until SIGTERM or SIGINT; then the engine stops
//! everything it started and the socket and pid file go.
//!
//! Module blocks go through the module catalog ([`crate::catalog`]): each type's build is found through F1 (a `__fetch`
//! child, pinned in `doze.lock`), its module started in the pool, and what it declares (manifest, attributes,
//! connection) is what the config and the views see. The instances doze sits in front of get a listener on their
//! declared address ([`crate::front`]): a connection wakes them.

use crate::catalog::{Catalog, Fetched};
use crate::ctl::{self, RunDir};
use crate::front::{Front, Fronted};
use crate::{Daemon, Labels, StackInfo};
use doze_config::{Instance, Kind, Spec};
use doze_engine::modules::Pinned;
use doze_engine::stack::Planner;
use doze_fetch::child::ChildDownloads;
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

/// A stack made ready to serve: its config, its engine (module instances planned through the catalog), and what the
/// views say of each instance.
#[derive(Debug)]
pub struct Assembled {
    pub cfg: doze_config::Config,
    pub engine: doze_engine::Engine,
    pub labels: BTreeMap<String, Labels>,
    /// Module instances whose module says doze does not sit in front of them.
    pub supervised: std::collections::BTreeSet<String>,
    /// Each module instance, for its data, console and client.
    pub modules: Modules,
}

/// Module instances by name.
pub type Modules = BTreeMap<String, Arc<doze_engine::modules::ModuleInstance>>;

impl Assembled {
    /// The instances doze sits in front of, for [`Front::open`].
    pub fn fronted(&self) -> Vec<Fronted> {
        fronted(&self.cfg, &self.labels, &self.supervised)
    }
}

/// Loads the stack at `config` with `catalog` (on a blocking thread: the catalog asks its modules as the config is
/// read) and builds its engine. `env` is read before this process's environment (a test's `DOZE_HOME`).
pub async fn assemble(
    config: &Path,
    version: &str,
    catalog: Arc<Catalog>,
    env: BTreeMap<String, String>,
) -> Result<Assembled, String> {
    let (path, version) = (config.to_string_lossy().into_owned(), version.to_owned());
    let (cfg, labels, (stack, modules), supervised) = tokio::task::spawn_blocking(move || {
        let cfg = load(&path, &version, catalog.as_ref(), env)?;
        let labels = labels(&cfg, catalog.as_ref());
        let stack = stack(&cfg, &catalog);
        let supervised = cfg
            .instances
            .iter()
            .filter(|i| i.kind() == Kind::Module)
            .filter(|i| {
                catalog
                    .manifest_for(&i.ty, &i.version)
                    .is_some_and(|m| m.supervised)
            })
            .map(|i| i.name.clone())
            .collect();
        Ok::<_, String>((cfg, labels, stack, supervised))
    })
    .await
    .map_err(|e| e.to_string())??;
    let engine = doze_engine::Engine::new(stack).map_err(|e| e.to_string())?;
    Ok(Assembled {
        cfg,
        engine,
        labels,
        supervised,
        modules,
    })
}

/// The engine's stack, each module instance planned through the catalog (which asks its module: a blocking thread's).
fn stack(
    cfg: &doze_config::Config,
    catalog: &Arc<Catalog>,
) -> (doze_engine::stack::Stack, Modules) {
    let (target, run_dir) = (doze_fetch::triple(), Path::new(&cfg.data_dir).join("run"));
    let modules = std::sync::Mutex::new(Modules::new());
    let planner = |i: &Instance| -> Arc<dyn Planner> {
        let (p, mi) = crate::planners::module(catalog, i, &target, &run_dir);
        if let Some(mi) = mi {
            crate::lock(&modules).insert(i.name.clone(), mi);
        }
        p
    };
    let wiring = doze_config::stack::Wiring {
        planner: &planner,
        extra_env: ca_env(Path::new(&cfg.home)),
    };
    let stack = doze_config::stack::to_stack(cfg, &wiring);
    let modules = modules
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (stack, modules)
}

#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
async fn run(config: &Path, version: &str) -> Result<(), String> {
    // Before the catalog starts a single module: a second daemon for this stack says so and nothing else runs.
    let (run, _alone) = no_daemon_yet(config, version)?;
    let catalog = Arc::new(catalog(config)?);
    let assembled = assemble(config, version, catalog.clone(), BTreeMap::new()).await?;
    let fronts = assembled.fronted();
    let Assembled {
        cfg,
        engine,
        labels,
        modules: instances,
        ..
    } = assembled;
    // Loading took a while: one started meanwhile still wins.
    refuse_if_running(&run)?;
    let info = StackInfo {
        listen: cfg.listen.clone(),
        dir: cfg.dir.clone(),
        loaded: Some(std::time::SystemTime::now()),
    };
    // The idle reaper and the health prober (Go's `RunReaper`, `RunHealthProber`), and the module pool's own.
    let background = engine.background();
    let modules = tokio::spawn(reap_modules(catalog.clone()));
    let front = Arc::new(Front::open(&engine, &fronts).await);
    let live = crate::live::Live::new(
        catalog.clone(),
        config,
        version,
        BTreeMap::new(),
        front.clone(),
    );
    let daemon = Daemon::new(engine.clone(), labels, info)
        .with_modules(instances)
        .with_live(live);
    let meters = daemon.keep_meters();
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
    front.close();
    modules.abort();
    meters.abort();
    drop(background);
    engine.shutdown().await;
    let _ = std::fs::remove_file(run.pid_file());
    Ok(())
}

/// Stops module processes nothing has called for a while, now and then.
async fn reap_modules(catalog: Arc<Catalog>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        catalog.pool.reap_idle().await;
    }
}

/// The stack at `path`, as the CLI loads it: this process's environment (`DOZE_VAR_*` included), doze-names' checks.
fn load(
    path: &str,
    version: &str,
    modules: &dyn doze_config::modules::Modules,
    env: BTreeMap<String, String>,
) -> Result<doze_config::Config, String> {
    with_options(version, modules, env, |opts| doze_config::load(path, opts))
}

/// One block read alone as if it were in the stack at `path` (Go's `Parse` of the add op's block): its references to
/// other instances do not resolve.
pub(crate) fn parse_block(
    block: &str,
    path: &str,
    version: &str,
    modules: &dyn doze_config::modules::Modules,
    env: BTreeMap<String, String>,
) -> Result<doze_config::Config, String> {
    with_options(version, modules, env, |opts| {
        doze_config::parse(block, path, opts, false)
    })
}

pub(crate) fn with_options(
    version: &str,
    modules: &dyn doze_config::modules::Modules,
    env: BTreeMap<String, String>,
    f: impl FnOnce(&doze_config::Options<'_>) -> Result<doze_config::Config, doze_config::Error>,
) -> Result<doze_config::Config, String> {
    let domains = Domains;
    let host = Overlay {
        os: doze_config::host::OsHost { domains: &domains },
        env: Arc::new(env),
    };
    let opts = doze_config::Options {
        host: &host,
        modules,
        vars: BTreeMap::new(),
        version: version.into(),
    };
    f(&opts).map_err(|e| e.to_string())
}

/// doze's CA for what it runs (Go's `caEnv`), when doze has made one.
fn ca_env(home: &Path) -> BTreeMap<String, String> {
    let tls = home.join("tls");
    if !tls.join("ca.pem").exists() {
        return BTreeMap::new();
    }
    doze_config::stack::ca_env(&tls.join("ca.pem"), &tls.join("bundle.pem"))
}

/// The catalog for the stack at `config`: module builds through a `__fetch` child of this very program, the lock
/// beside doze.hcl, and the lock's engine pins in front of the downloads modules ask for.
fn catalog(config: &Path) -> Result<Catalog, String> {
    let program = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
    let fetch = ChildDownloads {
        program,
        args: Vec::new(),
        env: Vec::new(),
    };
    let dir = config.parent().unwrap_or_else(|| Path::new("."));
    let lock = dir.join(doze_fetch::lock::FILE);
    let downloads = Arc::new(Pinned {
        inner: fetch.clone(),
        pins: crate::catalog::lock_pins(&lock),
    });
    let home = doze_os::home::doze_home().map_err(|e| format!("the doze home: {e}"))?;
    Ok(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Fetched(fetch)),
        downloads,
        lock,
        home,
    ))
}

/// What the config says about each instance, for the views; a module instance's variables, URL and client as its
/// module answers them.
fn labels(cfg: &doze_config::Config, cat: &Catalog) -> BTreeMap<String, Labels> {
    cfg.instances
        .iter()
        .map(|i| (i.name.clone(), label(i, cat)))
        .collect()
}

/// What the config says about one instance.
pub(crate) fn label(i: &Instance, cat: &Catalog) -> Labels {
    let run = match &i.spec {
        Spec::Command(c) => c.run.clone(),
        _ => String::new(),
    };
    let mut l = Labels {
        kind: match i.kind() {
            Kind::Module => String::new(),
            k => k.as_str().into(),
        },
        engine: i.ty.clone(),
        version: i.version.clone(),
        run,
        needs: i.deps.iter().map(|d| d.name.clone()).collect(),
        endpoint: doze_config::instance_addr(i, cat).unwrap_or_default(),
        domain: i.domains.first().cloned().unwrap_or_default(),
        paused: !i.enabled,
        ..Labels::default()
    };
    if i.kind() == Kind::Module
        && let Some(p) =
            crate::planners::as_module_sees_it(cat, i).and_then(|mi| cat.published(&mi))
    {
        l.connect = cat
            .manifest_for(&i.ty, &i.version)
            .and_then(|m| m.client)
            .map(|c| crate::client::view(&c, &l.endpoint, &p.url));
        (l.url, l.env_var, l.env, l.client) = (p.url, p.env_var, p.env, p.client);
    }
    l
}

/// The instances doze sits in front of: every module that is not supervised, and every app or service with a port
/// it does not bind itself.
fn fronted(
    cfg: &doze_config::Config,
    labels: &BTreeMap<String, Labels>,
    supervised: &std::collections::BTreeSet<String>,
) -> Vec<Fronted> {
    cfg.instances
        .iter()
        .filter_map(|i| front_of(i, labels.get(&i.name)?, supervised.contains(&i.name)))
        .collect()
}

/// Where doze listens for `i`, if it sits in front of it.
pub(crate) fn front_of(i: &Instance, l: &Labels, supervised: bool) -> Option<Fronted> {
    let fronted = match &i.spec {
        Spec::Command(c) => i.port > 0 && !c.direct,
        _ => i.kind() == Kind::Module && !supervised,
    };
    let addr = &l.endpoint;
    (i.enabled && fronted && !addr.is_empty() && !addr.starts_with("unix:")).then(|| Fronted {
        name: i.name.clone(),
        addr: addr.clone(),
    })
}

/// This process's machine, with some variables set first.
struct Overlay<'a> {
    os: doze_config::host::OsHost<'a>,
    env: Arc<BTreeMap<String, String>>,
}

impl doze_config::host::Host for Overlay<'_> {
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        let (os, env) = (self.os.env_lookup(), self.env.clone());
        Arc::new(move |name: &str| env.get(name).cloned().or_else(|| os(name)))
    }
    fn cwd(&self) -> PathBuf {
        self.os.cwd()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.os.home_dir()
    }
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        self.os.valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        self.os.route_domain(host)
    }
    fn check_certificate(&self, cert: &Path, key: &Path, domain: &str) -> Result<(), String> {
        self.os.check_certificate(cert, key, domain)
    }
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

/// The stack's run folder, made, when no daemon serves it yet: the data folder comes from doze.hcl read without its
/// modules (the shape pass), so nothing starts before this answers.
pub fn no_daemon_yet(config: &Path, version: &str) -> Result<(RunDir, std::fs::File), String> {
    let path = config.to_string_lossy();
    let shape = with_options(version, &doze_config::NoModules, BTreeMap::new(), |o| {
        doze_config::load_shape(&path, o)
    })?;
    let run = RunDir::of(Path::new(&shape.data_dir));
    doze_os::fs::private_dir(run.path()).map_err(|e| e.to_string())?;
    refuse_if_running(&run)?;
    // Held for this daemon's life (the OS drops it with the process): two started at once cannot both get here.
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(run.path().join("doze.daemon.lock"))
        .map_err(|e| e.to_string())?;
    match held.try_lock() {
        Ok(()) => Ok((run, held)),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "another doze daemon is starting for {}",
            run.path().display()
        )),
        Err(std::fs::TryLockError::Error(e)) => Err(e.to_string()),
    }
}

fn refuse_if_running(run: &RunDir) -> Result<(), String> {
    if ctl::running(run) {
        return Err(format!(
            "a daemon already serves {}",
            run.socket().display()
        ));
    }
    Ok(())
}
