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
    /// Fields this build does not know, written by a newer program of the same format, kept and written back. Go
    /// dropped them on every rewrite; that is how an older doze-aws once erased `ports` and every connection to the
    /// name was refused (`heal` in Go exists to repair it).
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Entry {
    /// The address, if it is one.
    pub fn addr(&self) -> Option<Ipv4Addr> {
        self.ip.parse().ok()
    }

    fn alive(&self) -> bool {
        u32::try_from(self.pid).is_ok_and(doze_os::process::alive)
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
        m.retain(|_, e| e.alive());
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
        if let Err(e) = std::fs::write(&tmp, text) {
            eprintln!("DIAG {:?} write tmp failed: {e:?}", self.owner);
            return Err(e.into());
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            eprintln!("DIAG {:?} rename failed: {e:?}", self.owner);
            return Err(e.into());
        }
        Ok(out)
    }

    /// The live entries, dead ones pruned, without the format stamp. A file this build cannot read gives none.
    pub fn snapshot(&self) -> BTreeMap<String, Entry> {
        self.update(|m| Ok(m.clone())).unwrap_or_default()
    }

    /// The address a host resolves to on this machine, or `None`. Answers for every peer's names, not only this
    /// process's: that is what lets any program serve the whole zone.
    pub fn resolve(&self, host: &str) -> Option<Ipv4Addr> {
        self.lookup(host).and_then(|e| e.addr())
    }

    /// The entry that owns `host`: an exact match, else the nearest registered ancestor (a name owns its subtree).
    ///
    /// That is how doze-aws answers `x70an6eshc.execute-api.ap-south-1.aws.harbour.doze`, minted at runtime and never
    /// claimable in advance. Two rules keep it from being a catch-all: the most specific claim wins (the walk starts
    /// at the full name), and the walk stops before the zone itself, so a name nobody claimed is still NXDOMAIN ("a
    /// typo fails as a name that does not exist rather than as a connection to the wrong thing").
    pub fn lookup(&self, host: &str) -> Option<Entry> {
        let m = self.snapshot();
        if !in_zone(host) {
            return crate::domains::lookup_domain(&m, host).map(|(e, _)| e);
        }
        let mut h = normalize(host);
        while h != crate::SUFFIX {
            if let Some(e) = m.get(&h) {
                return Some(e.clone());
            }
            let (_, rest) = h.split_once('.')?;
            h = rest.to_string();
        }
        None
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "registry_tests_more.rs"]
mod tests_more;
