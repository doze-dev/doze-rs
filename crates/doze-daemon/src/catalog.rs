//! The module catalog: what doze-config and the engine know of modules comes from the modules themselves. A module
//! type is resolved through F1 (a `__fetch` child resolves `doze/<type>` against the registry, pinned in `doze.lock`)
//! and run in the module pool; its manifest says whether it is versionless, its default port, whether doze sits in
//! front of it, its schema, its client; its `connection` and `attributes` calls say what an instance publishes. Nothing
//! here knows any module by name.
//!
//! doze-config asks synchronously while it loads (on a blocking thread of the daemon's runtime), so each question is
//! the module's answer awaited on the runtime's handle.

use crate::spec_tree;
use doze_config::modules::{Attributes, Call, Endpoint, Listener, ModuleInfo, Modules};
use doze_fetch::child::{ChildDownloads, Request};
use doze_module_host::BoxFuture;
use doze_module_host::{Locate, Pool, Running, Services};
use doze_module_proto::msg::hello::{Manifest, caps};
use doze_module_proto::msg::instance::{Address, Granted, Instance};
use doze_module_proto::network::MAIN;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Handle;

/// How long a module process may sit unused before the pool stops it (a Rust module starts in milliseconds).
const MODULE_IDLE: Duration = Duration::from_secs(120);

/// One stack's modules.
pub struct Catalog {
    rt: Handle,
    finder: Arc<dyn Finder>,
    /// The project's `doze.lock` (beside doze.hcl).
    lock: PathBuf,
    home: PathBuf,
    pub(crate) pool: Arc<Pool>,
    exes: Arc<Mutex<BTreeMap<String, PathBuf>>>,
    manifests: Mutex<BTreeMap<String, Manifest>>,
    failed: Mutex<BTreeMap<String, String>>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("lock", &self.lock)
            .finish_non_exhaustive()
    }
}

impl Catalog {
    /// A catalog for the project whose lock is `lock`, finding module builds through `finder`; downloads its modules
    /// ask for go through `downloads` (F1's child, with the lock's engine pins in front).
    pub fn new(
        rt: Handle,
        finder: Arc<dyn Finder>,
        downloads: Arc<dyn doze_module_host::Downloads>,
        lock: PathBuf,
        home: PathBuf,
    ) -> Catalog {
        let exes: Arc<Mutex<BTreeMap<String, PathBuf>>> = Arc::default();
        let found = exes.clone();
        let locate: Locate = Arc::new(move |engine| crate::lock(&found).get(engine).cloned());
        let services = Services {
            downloads,
            backends: None,
            notices: None,
        };
        Catalog {
            rt,
            finder,
            lock,
            home,
            pool: Arc::new(Pool::new(services, locate, MODULE_IDLE)),
            exes,
            manifests: Mutex::default(),
            failed: Mutex::default(),
        }
    }

    /// The manifest of a type already looked up.
    pub fn manifest(&self, ty: &str) -> Option<Manifest> {
        crate::lock(&self.manifests).get(ty).cloned()
    }

    /// Finds `ty`'s build for `version`'s major and starts its module (as `key`) for its hello; its manifest. A build
    /// already running as another key is not started twice.
    fn resolve(&self, key: &str, ty: &str, version: &str) -> Result<Manifest, String> {
        let major = major_of(version);
        let majors = if major.is_empty() {
            Vec::new()
        } else {
            vec![major.to_string()]
        };
        let exe = self.rt.block_on(self.finder.find(
            ty,
            &majors,
            (key == ty).then_some(self.lock.as_path()),
        ))?;
        {
            let mut exes = crate::lock(&self.exes);
            if key != ty && exes.get(ty) == Some(&exe) {
                return Err(format!(
                    "no {ty} build serves {major} (the one found is the one already running)"
                ));
            }
            exes.insert(key.to_string(), exe);
        }
        let m = self
            .rt
            .block_on(self.pool.get(key))
            .map_err(|e| format!("module {ty}: {}", e.message))?;
        Ok(m.manifest.clone())
    }

    fn running(&self, ty: &str, version: &str) -> Option<Arc<Running>> {
        let key = self.key(ty, version);
        self.manifest(&key)?;
        self.rt.block_on(self.pool.get(&key)).ok()
    }

    /// What one instance publishes: its connection (variable, URL, more variables) and, when the module offers them,
    /// attributes of its own.
    pub fn published(&self, i: &Instance) -> Option<Published> {
        let m = self.running(&i.engine, &i.version)?;
        let conn = self.rt.block_on(m.connection(i)).ok()?;
        let extras = if m.manifest.caps & caps::ATTRIBUTES != 0 {
            self.rt.block_on(m.attributes(i)).unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let mut env = conn.env.clone();
        if !conn.env_var.is_empty() {
            env.insert(conn.env_var.clone(), conn.url.clone());
        }
        Some(Published {
            env_var: conn.env_var,
            url: conn.url,
            env,
            extras,
            client: m
                .manifest
                .client
                .as_ref()
                .map(|c| c.program.clone())
                .unwrap_or_default(),
        })
    }

    /// Whether the instance's engine cannot listen on a socket file (its main listener is `tcp`, PLAN.md D13).
    pub fn main_is_tcp(&self, i: &Instance) -> bool {
        let Some(m) = self.running(&i.engine, &i.version) else {
            return false;
        };
        self.rt
            .block_on(m.network(i))
            .unwrap_or_default()
            .iter()
            .any(|l| l.name == MAIN && l.tcp)
    }

    /// The project's `doze.lock`.
    pub fn lock_path(&self) -> &Path {
        &self.lock
    }

    /// Every engine pin `doze.lock` holds (a module takes the ones for the engines it runs, whatever they are named).
    pub fn pins(&self) -> Vec<doze_module_proto::msg::run::Pin> {
        lock_pins(&self.lock)
    }
}

/// Where a module type's build comes from.
pub trait Finder: Send + Sync {
    /// The executable of the build of module `ty` that serves engine `majors`, pinned in the lock at `lock`; with no
    /// lock, resolved from the registry and recorded nowhere (a second build of a type: doze.lock keeps one pin per
    /// module source).
    fn find<'a>(
        &'a self,
        ty: &'a str,
        majors: &'a [String],
        lock: Option<&'a Path>,
    ) -> BoxFuture<'a, Result<PathBuf, String>>;
}

/// F1: a `__fetch` child resolves `doze/<type>` against the registry, pins it in doze.lock, and fetches and verifies the
/// build into the module cache.
#[derive(Debug, Clone)]
pub struct Fetched(pub ChildDownloads);

impl Finder for Fetched {
    fn find<'a>(
        &'a self,
        ty: &'a str,
        majors: &'a [String],
        lock: Option<&'a Path>,
    ) -> BoxFuture<'a, Result<PathBuf, String>> {
        Box::pin(async move {
            let req = Request::Module {
                source: ty.to_string(),
                majors: majors.to_vec(),
                lock: lock.map(Path::to_path_buf),
                persist: lock.is_some(),
            };
            let reply = self.0.ask(&req).await.map_err(|e| e.message)?;
            if let Some(e) = reply.error {
                return Err(e);
            }
            if reply.exe.is_empty() {
                return Err(format!("module {ty}: the fetch named no executable"));
            }
            Ok(PathBuf::from(reply.exe))
        })
    }
}

/// What a module says an instance publishes.
#[derive(Debug, Clone, Default)]
pub struct Published {
    pub env_var: String,
    pub url: String,
    /// Every variable a client sets (the main one included).
    pub env: BTreeMap<String, String>,
    pub extras: BTreeMap<String, doze_module_proto::msg::value::Value>,
    pub client: String,
}

/// Every engine pin of the lock at `path`, as the module protocol carries them (none when there is no lock).
pub fn lock_pins(path: &Path) -> Vec<doze_module_proto::msg::run::Pin> {
    let Ok(lock) = doze_fetch::lock::Lock::load(path) else {
        return Vec::new();
    };
    lock.engines
        .into_iter()
        .flat_map(|(engine, specs)| {
            specs
                .into_iter()
                .map(move |(spec, p)| doze_module_proto::msg::run::Pin {
                    engine: engine.clone(),
                    spec,
                    resolved: p.resolved,
                    hashes: p.hashes,
                })
        })
        .collect()
}

/// Records engine pins a module reported in the lock at `path` (Go's `up` wrote them), so the next start resolves
/// offline. A lock that cannot be read or written is said in the log and left alone: the start goes on.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
pub fn record_pins(path: &Path, pins: &[doze_module_proto::msg::run::Pin]) {
    // One writer at a time: two instances starting at once would each read, change and save, one losing the other's
    // pins (and the save's temporary file is named by the pid, so the second rename found it gone).
    static WRITING: Mutex<()> = Mutex::new(());
    let _one = crate::lock(&WRITING);
    let mut lock = if path.exists() {
        match doze_fetch::lock::Lock::load(path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("doze daemon: {} not updated: {e}", path.display());
                return;
            }
        }
    } else {
        doze_fetch::lock::Lock::default()
    };
    for p in pins {
        let pin = doze_fetch::lock::EnginePin {
            resolved: p.resolved.clone(),
            source: "mirror".into(),
            hashes: p.hashes.clone(),
        };
        lock.record_engine(&p.engine, &p.spec, pin);
    }
    if let Err(e) = lock.save(path, None) {
        eprintln!("doze daemon: {} not updated: {e}", path.display());
    }
}

/// The instance a module is asked about while the config loads: reached at `endpoint`, its backend there too.
pub fn instance_for(call: &Call<'_>, manifest: &Manifest) -> Instance {
    let addr = match call.endpoint {
        Endpoint::Tcp(a) => a.clone(),
        Endpoint::Unix(p) => format!("unix:{p}"),
    };
    let backend = match call.endpoint {
        Endpoint::Tcp(a) => Address::Tcp(a.clone()),
        Endpoint::Unix(p) => Address::Unix(p.clone()),
    };
    Instance {
        name: call.name.to_string(),
        engine: call.ty.to_string(),
        version: call.version.to_string(),
        data_dir: String::new(),
        socket_dir: String::new(),
        spec: spec_tree::tree(call.body, &manifest.schema),
        host: String::new(),
        listeners: BTreeMap::from([(MAIN.to_string(), granted(addr, backend))]),
        deps: Vec::new(),
        toolchain: None,
    }
}

/// A listener granted at `addr` whose engine listens at `backend`.
pub fn granted(addr: String, backend: Address) -> Granted {
    Granted {
        addr,
        url: String::new(),
        hosts: Vec::new(),
        backend,
    }
}

impl Modules for Catalog {
    fn lookup(&self, ty: &str, version: &str) -> Option<ModuleInfo> {
        if let Some(m) = self.manifest(ty) {
            return Some(info(&self.another_build(ty, version, m)));
        }
        if crate::lock(&self.failed).contains_key(ty) {
            return None;
        }
        match self.resolve(ty, ty, version) {
            Ok(m) => {
                let i = info(&m);
                crate::lock(&self.manifests).insert(ty.to_string(), m);
                Some(i)
            }
            Err(e) => {
                crate::lock(&self.failed).insert(ty.to_string(), e);
                None
            }
        }
    }

    fn cached(&self, ty: &str) -> Option<ModuleInfo> {
        self.manifest(ty).map(|m| info(&m))
    }

    fn lookup_error(&self, ty: &str) -> Option<String> {
        crate::lock(&self.failed).get(ty).cloned()
    }

    fn names(&self) -> Vec<String> {
        let mut names = doze_fetch::registry::Cache::new(&self.home)
            .modules(doze_fetch::registry::source::DEFAULT_NAMESPACE);
        names.extend(crate::lock(&self.manifests).keys().cloned());
        names.sort();
        names.dedup();
        names
    }

    fn check_support(&self, ty: &str, version: &str) -> Result<(), String> {
        let Some(m) = self.manifest_for(ty, version) else {
            return Ok(());
        };
        supports(ty, &m, version)
    }

    fn attributes(&self, call: &Call<'_>) -> Attributes {
        let Some(m) = self.manifest_for(call.ty, call.version) else {
            return Attributes::default();
        };
        let Some(p) = self.published(&instance_for(call, &m)) else {
            return Attributes::default();
        };
        Attributes {
            env_var: p.env_var,
            url: p.url,
            extras: p
                .extras
                .iter()
                .map(|(k, v)| (k.clone(), spec_tree::back(v)))
                .collect(),
        }
    }

    fn listeners(&self, call: &Call<'_>, _port: u16) -> Vec<Listener> {
        let Some(m) = self.manifest_for(call.ty, call.version) else {
            return Vec::new();
        };
        let Some(r) = self.running(call.ty, call.version) else {
            return Vec::new();
        };
        let i = instance_for(call, &m);
        self.rt
            .block_on(r.network(&i))
            .unwrap_or_default()
            .into_iter()
            .filter(|l| l.name != MAIN)
            .map(|l| Listener {
                name: l.name,
                port: l.port,
            })
            .collect()
    }
}

/// What the config needs of a manifest.
pub fn info(m: &Manifest) -> ModuleInfo {
    ModuleInfo {
        versionless: m.versions.is_empty(),
        default_port: m.default_port,
        supervised: m.supervised,
    }
}

/// Whether the module serves `version`: one of its majors, or an exact version under one.
pub fn supports(ty: &str, m: &Manifest, version: &str) -> Result<(), String> {
    if m.versions.is_empty() || version.is_empty() {
        return Ok(());
    }
    let served = m
        .versions
        .iter()
        .any(|v| version == v || version.starts_with(&format!("{v}.")));
    if served {
        return Ok(());
    }
    Err(format!(
        "{ty} {version:?} is not a version the {ty} module serves (it serves {})",
        m.versions.join(", ")
    ))
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
pub(crate) mod tests;

#[path = "catalog_builds.rs"]
mod builds;
use builds::major_of;
