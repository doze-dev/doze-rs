//! The module catalog: what doze-config and the engine know of modules comes from the modules themselves. A module
//! type is resolved through F1 (a `__fetch` child resolves `doze/<type>` against the registry, pinned in `doze.lock`)
//! and run in the module pool; its manifest says whether it is versionless, its default port, whether doze sits in
//! front of it, its schema, its client; its `connection` and `attributes` calls say what an instance publishes. Nothing
//! here knows any module by name.
//!
//! doze-config asks synchronously while it loads (on a blocking thread of the daemon's runtime), so each question is
//! the module's answer awaited on the runtime's handle.

use doze_fetch::child::{ChildDownloads, Request};
use doze_module_host::BoxFuture;
use doze_module_host::{Locate, Pool, Running, Services};
use doze_module_proto::msg::hello::{Manifest, caps};
use doze_module_proto::msg::instance::{Address, Instance};
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
    /// The engine a module taking its own connections boots and counts them through, once it exists.
    wire: Arc<WireHost>,
    /// What fetches a service's `download` (none: a block with one says nothing fetches it).
    service_downloads: Option<Arc<dyn doze_engine::Fetch>>,
}

/// The host's side of `wire.accept` (D2): a module handed a client connection asks the engine to boot the instance
/// when the client's first packets call for it, and counts the connection while it is spliced.
#[derive(Debug, Default)]
pub struct WireHost {
    engine: std::sync::OnceLock<doze_engine::Engine>,
}

impl doze_module_host::Backends for WireHost {
    fn boot<'a>(
        &'a self,
        instance: &'a str,
    ) -> BoxFuture<'a, Result<Address, doze_module_proto::msg::error::Error>> {
        Box::pin(async move {
            use doze_module_proto::msg::error::{Error, ErrorKind};
            let e = self
                .engine
                .get()
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "the daemon is starting"))?;
            let ep = e
                .wake(instance, doze_engine::Cause::Connection)
                .await
                .map_err(|err| Error::new(ErrorKind::Unavailable, err.to_string()))?;
            Ok(match ep.backend.strip_prefix("unix:") {
                Some(p) => Address::Unix(p.to_string()),
                None => Address::Tcp(ep.backend),
            })
        })
    }
    fn acquire(&self, instance: &str) {
        if let Some(e) = self.engine.get() {
            e.acquire(instance);
        }
    }
    fn release(&self, instance: &str) {
        if let Some(e) = self.engine.get() {
            e.release(instance);
        }
    }
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
        let wire = Arc::new(WireHost::default());
        let services = Services {
            downloads,
            backends: Some(wire.clone()),
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
            wire,
            service_downloads: None,
        }
    }

    /// Services' downloads fetched by `f`. Before the catalog is shared.
    #[must_use]
    pub fn with_service_downloads(mut self, f: Arc<dyn doze_engine::Fetch>) -> Catalog {
        self.service_downloads = Some(f);
        self
    }

    /// What fetches a service's `download`.
    pub fn service_downloads(&self) -> Option<Arc<dyn doze_engine::Fetch>> {
        self.service_downloads.clone()
    }

    /// The engine modules taking their own connections boot and count them through (once; before the first
    /// connection).
    pub fn attach(&self, engine: &doze_engine::Engine) {
        let _ = self.wire.engine.set(engine.clone());
    }

    /// Every listener the instance's module declares (`network`), the main one included; empty where it declares none.
    pub fn network(&self, i: &Instance) -> Vec<doze_module_proto::msg::instance::Listener> {
        let Some(r) = self.running(&i.engine, &i.version) else {
            return Vec::new();
        };
        self.rt.block_on(r.network(i)).unwrap_or_default()
    }

    /// The running module for a type (and major), if it is one this catalog found.
    pub async fn module_of(&self, key: &str) -> Option<Arc<Running>> {
        self.pool.get(key).await.ok()
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

#[path = "catalog_lock.rs"]
mod lock;
pub use lock::{lock_pins, record_pins};

#[path = "catalog_config.rs"]
mod config;
pub use config::{granted, info, instance_for, supports};

#[cfg(test)]
#[path = "catalog_tests.rs"]
pub(crate) mod tests;

#[path = "catalog_builds.rs"]
mod builds;
use builds::major_of;
