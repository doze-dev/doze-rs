//! The shared registry: `<home>/names.json`, written by every doze program and read by whichever serves the
//! resolver. The same file Go's doze-names writes, the same way, so Go and Rust programs share one zone.
//!
//! Liveness is by pid, not by clean shutdown: a crashed process runs no shutdown hook. Every read prunes entries
//! whose process is gone, so the file heals itself and a `kill -9` costs nothing.

use crate::pool::Net;
use crate::{Error, in_zone, normalize};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

/// The registry's file name inside the doze home.
pub const FILE_NAME: &str = "names.json";

/// The registry format this build reads and writes. Three programs released separately share the file, so each
/// write stamps it and each read checks it: a build that meets a newer file stops and says so rather than rewriting
/// names it does not understand. Raise it only when an older build could no longer read the file correctly.
pub const FORMAT_VERSION: u32 = 2;

/// The entry that records the format. Beside the names rather than in a wrapper around them, so a build from before
/// the version existed still reads the file (it sees one more entry it has no use for).
pub(crate) const FORMAT_KEY: &str = "_format";

/// One registered name (or bookkeeping entry), as the file has it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub pid: i64,
    /// "doze", "doze-aws" or "doze-kafka".
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub tier: String,
    /// Where the front door proxies this name ("host:port"); empty: it resolves but is not fronted.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// A certificate the person brought for a domain, served instead of one minted from the doze CA.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cert: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// Where an address is translated, not bound (macOS): each public port and the private port it is served on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ports: BTreeMap<String, u16>,
    /// Set only on the entry that records the file's format.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub format: u32,
    /// When (Unix seconds) a stopped stack left the name parked ([`Registry::park`]): it still resolves, to the same
    /// address, though nothing holds it. 0 for a held name.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub parked: u64,
    /// Fields this build does not know, written by a newer program of the same format, kept and written back. Go
    /// dropped them on every rewrite; that is how an older doze-aws once erased `ports` and every connection to the
    /// name was refused (`heal` in Go exists to repair it).
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

/// How long a parked name keeps resolving after its stack stopped: across a `doze down` and the next `up`, and a
/// night between them, but not for good (a project moved or deleted leaves nothing that answers for long).
pub const PARKED_FOR: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Entry {
    /// The address, if it is one.
    pub fn addr(&self) -> Option<Ipv4Addr> {
        self.ip.parse().ok()
    }

    /// Whether this is a parked name ([`Registry::park`]): held by no process, resolving until it expires or
    /// someone claims it.
    pub fn is_parked(&self) -> bool {
        self.pid == 0 && self.parked != 0
    }

    /// Whether the entry is kept: its process runs, or it is parked and not yet expired. Pid 0 is never a live
    /// process, so a build from before parking prunes a parked name as a dead one (it then answers NXDOMAIN, as all
    /// builds did before).
    fn kept_at(&self, now: u64) -> bool {
        u32::try_from(self.pid).is_ok_and(doze_os::process::alive)
            || (self.is_parked() && now < self.parked.saturating_add(PARKED_FOR.as_secs()))
    }
}

/// A handle on the shared name file.
#[derive(Debug, Clone)]
pub struct Registry {
    path: PathBuf,
    pub(crate) owner: String,
    pub(crate) pid: u32,
    pub(crate) net: Net,
}

impl Registry {
    /// A handle on the registry in `home`, written as `owner`. Nothing is created until something is claimed.
    pub fn open(home: &Path, owner: &str) -> Registry {
        Registry {
            path: home.join(FILE_NAME),
            owner: owner.to_string(),
            pid: std::process::id(),
            net: Net::native(),
        }
    }

    /// The same registry on another network: for tests, and for a caller that must plan for a machine other than
    /// this one.
    pub fn with_net(mut self, net: Net) -> Registry {
        self.net = net;
        self
    }

    /// The network this registry gives addresses on.
    pub fn net(&self) -> Net {
        self.net
    }

    /// The file's location.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` on the names under an exclusive lock, after pruning dead entries, and writes the result back
    /// atomically. Every read and write goes through here, so the prune is never skipped. A corrupt file starts
    /// clean (every running process re-registers); a newer one is left untouched and refused.
    pub(crate) fn update<T>(
        &self,
        f: impl FnOnce(&mut BTreeMap<String, Entry>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // On a sidecar, so the rename below cannot swap the file out from under the lock (Go: flock on .lock).
        let _lock = doze_os::fs::lock(&self.path.with_extension("json.lock"))?;
        let mut m: BTreeMap<String, Entry> = match std::fs::read(&self.path) {
            Ok(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        if let Some(stamp) = m.get(FORMAT_KEY)
            && stamp.format > FORMAT_VERSION
        {
            let path = self.path.display().to_string();
            return Err(Error::NewerFormat {
                path,
                have: stamp.format,
                want: FORMAT_VERSION,
            });
        }
        m.remove(FORMAT_KEY);
        let now = now_secs();
        m.retain(|_, e| e.kept_at(now));
        let out = f(&mut m)?;
        // The stamp carries this pid so an older build, which prunes every entry whose process is gone, keeps it
        // while a writer that understands it runs.
        let stamp = Entry {
            pid: i64::from(self.pid),
            owner: self.owner.clone(),
            format: FORMAT_VERSION,
            ..Entry::default()
        };
        m.insert(FORMAT_KEY.to_string(), stamp);
        let mut text = serde_json::to_string_pretty(&m).map_err(std::io::Error::other)?;
        text.push('\n');
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(out)
    }

    /// The live entries, dead ones pruned, without the format stamp or parked names. A file this build cannot read
    /// gives none.
    pub fn snapshot(&self) -> BTreeMap<String, Entry> {
        let mut m = self.entries();
        m.retain(|_, e| !e.is_parked());
        m
    }

    /// The entries as they stand, parked names included.
    ///
    /// Read without the lock and without writing: the file is only ever replaced by a rename, so a read is of one
    /// whole version. Only when it holds something to prune (or cannot be read) does it go through [`Registry::update`],
    /// which writes. Every read used to: each DNS question, each request at the front door and each TLS handshake took
    /// the lock and rewrote the file, and each rewrite told the network daemon the table might have changed.
    fn entries(&self) -> BTreeMap<String, Entry> {
        self.read_clean()
            .unwrap_or_else(|| self.update(|m| Ok(m.clone())).unwrap_or_default())
    }

    /// The file's entries when it has nothing to prune; `None` when it has, or cannot be read, or is newer than this
    /// build (which [`Registry::update`] refuses), or carries no format stamp yet (which it adds, once).
    fn read_clean(&self) -> Option<BTreeMap<String, Entry>> {
        let raw = match std::fs::read(&self.path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(BTreeMap::new()),
            Err(_) => return None,
        };
        let mut m: BTreeMap<String, Entry> = serde_json::from_slice(&raw).ok()?;
        if m.remove(FORMAT_KEY)
            .is_none_or(|s| s.format != FORMAT_VERSION)
        {
            return None;
        }
        let now = now_secs();
        m.values().all(|e| e.kept_at(now)).then_some(m)
    }

    /// The address a host resolves to on this machine, or `None`. Answers for every peer's names, not only this
    /// process's: that is what lets any program serve the whole zone. A parked name resolves too.
    pub fn resolve(&self, host: &str) -> Option<Ipv4Addr> {
        let m = self.entries();
        let e = if in_zone(host) {
            zone_owner(&m, host).map(|(e, _)| e)
        } else {
            crate::domains::lookup_domain(&m, host).map(|(e, _)| e)
        };
        e.and_then(|e| e.addr())
    }

    /// Parks this process's names in the zone, for a stack that is stopping: each keeps resolving to its address for
    /// [`PARKED_FOR`], or until a process claims it, though nothing holds it.
    ///
    /// Why: between a stack's `down` and its next `up` its names used to be NXDOMAIN, and a client that asked in that
    /// moment (an app retrying, a browser tab) was told the name does not exist. A cache may keep that answer (Go's
    /// carried no SOA to bound it), and so `kafka.larder.doze` failed after the stack was back. A parked name answers
    /// with the address it will have again (addresses are hashed from the name), so a connection meanwhile is refused,
    /// like any stopped service's port, and the next one after `up` works with nothing to forget.
    ///
    /// Only names with an address of their own are parked: one on a shared address (127.0.0.1 without machine setup)
    /// is reached by its port, and another stack may take that port while this one is down. Domains outside the zone
    /// are not parked either: while no stack serves `dev.acme.com` it is the real one's. Nothing that fronts or
    /// translates is kept (route, certificate, ports), so the front door and the network daemon see no parked name.
    pub fn park(&self) -> Result<usize, Error> {
        let (pid, now, net) = (i64::from(self.pid), now_secs(), self.net);
        self.update(|m| {
            let mut n = 0;
            for (host, e) in m.iter_mut() {
                let own = e.addr().is_some_and(|ip| net.in_pool(ip));
                if e.pid == pid && own && in_zone(host) && !host.starts_with('_') {
                    *e = Entry {
                        ip: std::mem::take(&mut e.ip),
                        owner: std::mem::take(&mut e.owner),
                        tier: std::mem::take(&mut e.tier),
                        parked: now,
                        ..Entry::default()
                    };
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// The entry that owns `host`: an exact match, else the nearest registered ancestor (a name owns its subtree).
    ///
    /// That is how doze-aws answers `x70an6eshc.execute-api.ap-south-1.aws.harbour.doze`, minted at runtime and never
    /// claimable in advance. Two rules keep it from being a catch-all: the most specific claim wins (the walk starts
    /// at the full name), and the walk stops before the zone itself, so a name nobody claimed is still NXDOMAIN ("a
    /// typo fails as a name that does not exist rather than as a connection to the wrong thing").
    ///
    /// The front door answers such a name over https too, with a certificate for the name itself ([`crate::certs`]).
    pub fn lookup(&self, host: &str) -> Option<Entry> {
        let m = self.snapshot();
        if !in_zone(host) {
            return crate::domains::lookup_domain(&m, host).map(|(e, _)| e);
        }
        zone_owner(&m, host).map(|(e, _)| e)
    }
}

/// The entry owning a name in the zone and the registered name it was found under: [`Registry::lookup`]'s walk.
pub(crate) fn zone_owner(m: &BTreeMap<String, Entry>, host: &str) -> Option<(Entry, String)> {
    let mut h = normalize(host);
    while h != crate::SUFFIX {
        if let Some(e) = m.get(&h) {
            return Some((e.clone(), h));
        }
        let (_, rest) = h.split_once('.')?;
        h = rest.to_string();
    }
    None
}

#[cfg(test)]
#[path = "registry_tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "registry_tests_more.rs"]
mod tests_more;
