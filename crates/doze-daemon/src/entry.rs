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
use crate::names::{BindError, Zone};
use crate::planners::Grant;
use crate::{Daemon, Labels, StackInfo};
use doze_config::{Instance, Kind};
use doze_engine::modules::Pinned;
use doze_engine::stack::Planner;
use doze_fetch::child::ChildDownloads;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

/// The argument that makes `__daemon` check doze.hcl through its modules once (the `lint` op's pass) and print the
/// answer as one JSON line, serving nothing: `doze lint` with no daemon running.
pub const LINT_ARG: &str = "--lint";

/// Runs the daemon for `args` (what follows `__daemon`); its exit code.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
pub fn main(args: &[String], version: &str) -> ExitCode {
    let config = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map_or_else(|| PathBuf::from("doze.hcl"), PathBuf::from);
    // Small on purpose: a stack daemon mostly waits (sockets, timers, child pipes), and the default (a worker per core,
    // 512 blocking threads) left it at ~40 threads. The blocking work is short (loading doze.hcl, a file lock, a glob
    // walk, reading a log batch), none waits on another, so four run it; more queue a moment.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("doze daemon: runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    if args.iter().any(|a| a == LINT_ARG) {
        return crate::lint::main(&rt, &config, version);
    }
    match rt.block_on(run(&config, version)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("doze daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A stack made ready to serve: its config, its engine (module instances planned through the catalog), what the
/// views say of each instance, and its names: each instance doze fronts is placed and bound before the engine is
/// built, because what its module is told (its grant) is where it was put.
#[derive(Debug)]
pub struct Assembled {
    pub cfg: doze_config::Config,
    pub engine: doze_engine::Engine,
    pub labels: BTreeMap<String, Labels>,
    /// Module instances whose module says doze does not sit in front of them.
    pub supervised: std::collections::BTreeSet<String>,
    /// Each module instance, for its data, console and client.
    pub modules: Modules,
    /// The instances doze sits in front of.
    pub fronted: Vec<Fronted>,
    /// The stack in the zone (names off: claims nothing).
    pub zone: Arc<Zone>,
    /// Client TLS for modules that take their own connections (the `tls` block).
    pub tls: Option<crate::tls::WireTls>,
    bound: BTreeMap<String, doze_bind::Bound>,
}

/// Module instances by name.
pub type Modules = BTreeMap<String, Arc<doze_engine::modules::ModuleInstance>>;

impl Assembled {
    /// The instances doze sits in front of.
    pub fn fronted(&self) -> Vec<Fronted> {
        self.fronted.clone()
    }

    /// Starts serving every listener bound at assembly (once; a second call opens nothing), and joins the zone's
    /// resolver and front door.
    pub fn open_front(&mut self) -> Front {
        let bound = std::mem::take(&mut self.bound);
        Front::open(&self.engine, self.zone.clone(), &self.fronted, bound)
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
    let placed = tokio::task::spawn_blocking(move || {
        let cfg = load(&path, &version, catalog.as_ref(), env)?;
        let supervised: std::collections::BTreeSet<String> = cfg
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
        let zone = Arc::new(Zone::for_config(&cfg)?);
        let tls = crate::tls::for_config(&cfg)?;
        let run_dir = Path::new(&cfg.data_dir).join("run");
        let fronted: Vec<Fronted> = cfg
            .instances
            .iter()
            .filter_map(|i| {
                fronted_one(i, catalog.as_ref(), supervised.contains(&i.name), &run_dir)
            })
            .collect();
        let (grants, bound, unreachable) = bind_all(&zone, &fronted, &run_dir)?;
        zone.record_domains(&app_domains(&cfg));
        let mut labels = labels(&cfg, catalog.as_ref(), &as_bound(&grants, &bound));
        for (name, b) in &bound {
            if let Some(l) = labels.get_mut(name) {
                bound_labels(l, b.instances.get(name), &zone, name);
            }
        }
        let stack = stack(&cfg, &catalog, &grants);
        let unbound = (bound, unreachable);
        Ok::<_, String>((cfg, labels, stack, supervised, fronted, zone, tls, unbound))
    })
    .await
    .map_err(|e| e.to_string())??;
    let (cfg, labels, (stack, modules), supervised, fronted, zone, tls, (bound, unreachable)) =
        placed;
    let engine = doze_engine::Engine::new(stack).map_err(|e| e.to_string())?;
    for (name, why) in &unreachable {
        engine
            .set_unreachable(name, why)
            .map_err(|e| e.to_string())?;
    }
    Ok(Assembled {
        cfg,
        engine,
        labels,
        supervised,
        modules,
        fronted,
        zone,
        tls,
        bound,
    })
}

/// What each fronted instance is granted, the listeners bound, and the instances none of whose listeners could be
/// opened, each with why (an address something else holds). A name another program holds stops the stack.
type Binds = (
    BTreeMap<String, Grant>,
    BTreeMap<String, doze_bind::Bound>,
    Vec<(String, String)>,
);

/// Grants and binds every fronted instance of the plan (see [`Binds`]).
fn bind_all(zone: &Zone, fronted: &[Fronted], run_dir: &Path) -> Result<Binds, String> {
    let plan = zone.plan(fronted)?;
    let mut grants = BTreeMap::new();
    let mut bound = BTreeMap::new();
    let mut unreachable = Vec::new();
    for f in fronted {
        grants.insert(f.name.clone(), zone.grant(&plan, f, run_dir));
        match zone.bind(&plan, f, run_dir) {
            Ok(b) => {
                bound.insert(f.name.clone(), b);
            }
            Err(BindError::Held(e)) => return Err(e),
            // Go refused to start the stack. The rest of it runs here; this instance is failed with the reason (which
            // names the address), and every wake of it fails with that, so `status` and `up` say so.
            Err(e) => {
                eprintln_daemon(&e.to_string());
                unreachable.push((f.name.clone(), e.to_string()));
            }
        }
    }
    Ok((grants, bound, unreachable))
}

#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
fn eprintln_daemon(line: &str) {
    eprintln!("doze daemon: {line}");
}

/// Every app's own domains (`dev.acme.com`), for the routes record.
fn app_domains(cfg: &doze_config::Config) -> Vec<String> {
    cfg.instances
        .iter()
        .filter(|i| i.enabled)
        .flat_map(|i| i.domains.iter().cloned())
        .collect()
}

/// The grants as the views say them: a routed listener whose port was taken was bound on another, and a link names that one.
fn as_bound(
    grants: &BTreeMap<String, Grant>,
    bound: &BTreeMap<String, doze_bind::Bound>,
) -> BTreeMap<String, Grant> {
    grants
        .iter()
        .map(|(name, g)| {
            let at = bound.get(name).and_then(|b| b.instances.get(name));
            (
                name.clone(),
                at.map_or_else(|| g.clone(), |b| at_ports(g, &bound_ports(b))),
            )
        })
        .collect()
}

/// What binding says of an instance for the views: where it really listens, and a name of its another program holds.
fn bound_labels(l: &mut Labels, b: Option<&doze_bind::BoundInstance>, zone: &Zone, name: &str) {
    if let Some(main) = b.and_then(|b| b.listeners.get(doze_module_proto::network::MAIN)) {
        l.endpoint = main.public.to_string();
        l.bind = l.endpoint.clone();
    }
    if let Some(who) = zone.held(name) {
        l.name_held = who;
    }
}

/// The engine's stack, each module instance planned through the catalog (which asks its module: a blocking thread's)
/// with what the zone granted it.
fn stack(
    cfg: &doze_config::Config,
    catalog: &Arc<Catalog>,
    grants: &BTreeMap<String, Grant>,
) -> (doze_engine::stack::Stack, Modules) {
    let target = doze_fetch::triple();
    let modules = std::sync::Mutex::new(Modules::new());
    let planner = |i: &Instance| -> Arc<dyn Planner> {
        let (p, mi) = crate::planners::module(catalog, i, &target, grants.get(&i.name));
        if let Some(mi) = mi {
            crate::lock(&modules).insert(i.name.clone(), mi);
        }
        p
    };
    let wiring = doze_config::stack::Wiring {
        planner: &planner,
        extra_env: crate::names::ca_env(cfg),
        fetch: catalog.service_downloads(),
    };
    let stack = doze_config::stack::to_stack(cfg, &wiring);
    let modules = modules
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (stack, modules)
}

#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
async fn run(config: &Path, version: &str) -> Result<(), String> {
    // The stop handlers first: once the socket or the pid file exists, a stop must be a graceful one, not the
    // default SIGTERM that ends the process mid-way (embed.rs on macOS: stopped by the signal, not after it).
    let stop = doze_os::signal::install().map_err(|e| format!("catching stop signals: {e}"))?;
    // Before the catalog starts a single module: a second daemon for this stack says so and nothing else runs.
    let (run, _alone) = no_daemon_yet(config, version)?;
    #[cfg(debug_assertions)]
    slow_start::slow_start_for_tests().await;
    let catalog = Arc::new(catalog(config)?);
    let mut assembled = assemble(config, version, catalog.clone(), BTreeMap::new()).await?;
    let front = Arc::new(assembled.open_front());
    let Assembled {
        cfg,
        engine,
        mut labels,
        modules: instances,
        zone,
        tls,
        ..
    } = assembled;
    for (name, l) in &mut labels {
        l.name_only = front.name_only(name);
    }
    catalog.attach(&engine);
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
    daemon.answer_front(&front, tls);
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
    let asked = stop.requested().await;
    eprintln!(
        "doze daemon: stopping ({})",
        asked.map_or_else(|e| e.to_string(), |()| "asked to".into())
    );
    serving.stop();
    // Before the front closes, which releases every name: parked, they keep resolving until the stack is back.
    zone.park();
    front.close();
    modules.abort();
    meters.abort();
    drop(background);
    engine.shutdown().await;
    zone.close().await;
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

/// The catalog for the stack at `config`: module builds through a `__fetch` child of this very program, the lock
/// beside doze.hcl, and the lock's engine pins in front of the downloads modules ask for.
pub(crate) fn catalog(config: &Path) -> Result<Catalog, String> {
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
    let services = crate::downloads::ServiceDownloads {
        child: fetch.clone(),
        lock: lock.clone(),
        home: home.clone(),
    };
    Ok(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Fetched(fetch)),
        downloads,
        lock,
        home,
    )
    .with_service_downloads(Arc::new(services)))
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
        Ok(()) => {
            // This daemon's alone now: what it starts with is recorded for `doze sync` (a failure only loses that).
            if let Err(e) = ctl::write_blocks(&run, &shape) {
                eprintln!("doze daemon: could not record the blocks it starts with: {e}");
            }
            Ok((run, held))
        }
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

#[path = "entry_load.rs"]
mod load;
use load::load;
pub(crate) use load::{parse_block, with_options};

#[cfg(debug_assertions)]
#[path = "entry_slow_start.rs"]
mod slow_start;
#[cfg(debug_assertions)]
pub use slow_start::SLOW_START_VAR;

#[path = "entry_labels.rs"]
mod labels;
use labels::labels;
pub(crate) use labels::{at_ports, bound_ports, console_of, fronted_one, label};
