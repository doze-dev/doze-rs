//! What doze-config asks of the catalog while it loads: a module's info, whether it serves a version, an instance's
//! attributes and listeners.

use super::{Catalog, major_of};
use crate::spec_tree;
use doze_config::modules::{Attributes, Call, Endpoint, Listener, ModuleInfo, Modules};
use doze_module_proto::msg::hello::Manifest;
use doze_module_proto::msg::instance::{Address, Granted, Instance};
use doze_module_proto::network::MAIN;
use std::collections::BTreeMap;

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
        dir: call.dir.to_string(),
    }
}

/// A listener other than the main one, granted where nothing fronts it: at its declared `port` on `host`, its engine
/// listening there itself; a port of 0 is a loopback port the engine chooses at each start
/// ([`doze_engine::modules::ANY_LOOPBACK`]).
fn granted_at(host: &str, port: u16) -> Granted {
    let addr = if port == 0 {
        doze_engine::modules::ANY_LOOPBACK.to_string()
    } else {
        doze_config::join_host_port(host, &port.to_string())
    };
    granted(addr.clone(), Address::Tcp(addr))
}

impl Catalog {
    /// [`instance_for`], with every other listener its module declares (`network`: temporal's `ui`, kafka's
    /// `registry`) granted at its declared port: what the module sees before (or without) a front, so nothing it
    /// declared is missing from its instance.
    pub fn declared(&self, call: &Call<'_>, manifest: &Manifest) -> Instance {
        let mut i = instance_for(call, manifest);
        // The others are on the main one's host; loopback for a socket.
        let host = match call.endpoint {
            Endpoint::Tcp(a) => doze_config::split_host_port(a)
                .map(|(h, _)| h)
                .filter(|h| !h.is_empty()),
            Endpoint::Unix(_) => None,
        }
        .unwrap_or_else(|| "127.0.0.1".into());
        for l in self.network(&i) {
            if l.name != MAIN {
                i.listeners.insert(l.name, granted_at(&host, l.port));
            }
        }
        i
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
        supports(ty, &m, version).map_err(|refused| {
            let key = format!("{ty}@{}", major_of(version));
            let failed = crate::lock(&self.failed).get(&key).cloned();
            unserved(ty, version, refused, failed.as_deref())
        })
    }

    fn attributes(&self, call: &Call<'_>) -> Attributes {
        let Some(m) = self.manifest_for(call.ty, call.version) else {
            return Attributes::default();
        };
        let Some(p) = self.published(&self.declared(call, &m)) else {
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

/// Why `version` cannot run, when the type's first build does not serve it: the lookup of a build for its major failed
/// (the registry unreachable, a bad signature) says that, not that no build serves it (D1 audit item 11).
pub(crate) fn unserved(ty: &str, version: &str, refused: String, failed: Option<&str>) -> String {
    match failed {
        Some(why) if !(why.starts_with("no ") && why.contains(" build serves ")) => {
            let major = major_of(version);
            format!("{ty} {version:?}: the {ty} build for {major} could not be had: {why}")
        }
        _ => refused,
    }
}
