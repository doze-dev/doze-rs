//! Module builds from the signed registry (Go's `internal/modules`): which release a project runs, fetched, verified
//! and cached under `<home>/modules/<ns>/<name>/<version>-<triple>/`, and pinned in `doze.lock`.
//!
//! - A pin wins, so a moving channel cannot drift; its gates (the doze it needs, the engine majors) run offline, and a
//!   warm cache touches no network at all.
//! - With no pin, the newest release that fits ([`select::newest_that_fits`]) is taken and pinned.
//! - A pin to an older module protocol (a `doze.lock` written by the Go doze, protocol 1) is resolved again as if
//!   there were no pin, and the new pin replaces it: the Rust doze cannot run a protocol-1 module, and the lock is the
//!   project's to carry forward. A pin to a newer protocol says to update doze.
//! - When the registry cannot be reached and nothing is pinned, the newest build this machine has will do, unpinned.

use crate::lock::{Lock, Pin};
use crate::mirror::{self, Want};
use crate::registry::index::{self, Index, Release};
use crate::registry::select::{self, Fit, Here};
use crate::registry::source::Source;
use crate::registry::{self, FetchError};
use std::path::{Path, PathBuf};

/// The file beside a cached module build that names the protocol its verified release speaks.
pub const PROTOCOL_FILE: &str = ".doze-module-protocol";

/// Where module builds come from and where they are kept.
#[derive(Debug, Clone)]
pub struct Modules {
    pub registry: registry::Config,
    /// The doze home: builds are kept under `<home>/modules`.
    pub home: PathBuf,
    /// This machine's platform triple (`aarch64-apple-darwin`).
    pub triple: String,
    /// The module protocol this doze speaks.
    pub protocol: i64,
    /// This doze's version (`0.3.0`, or a development build).
    pub doze: String,
}

/// A module build ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Its executable.
    pub exe: PathBuf,
    pub version: String,
    /// What a person should hear: a new pin, a pin replaced, the cache used offline.
    pub notes: Vec<String>,
}

impl Modules {
    fn here(&self) -> Here<'_> {
        Here {
            protocol: self.protocol,
            doze: &self.doze,
        }
    }

    /// The cache folder of one build. `version` must be [`crate::lock::plain`] (checked where it comes in: the lock,
    /// the index); a version that is not stays under the cache as one escaped component.
    pub fn content_dir(&self, src: &Source, version: &str) -> PathBuf {
        let version = if crate::lock::plain(version) {
            version.to_string()
        } else {
            version.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        };
        self.home
            .join("modules")
            .join(src.ns())
            .join(src.name())
            .join(format!("{version}-{}", self.triple))
    }

    /// The build of `src` for engine `majors`, pinned in `lock` (a new or replaced pin marks it dirty; the caller
    /// decides whether to save it: read commands never write the lock).
    pub fn resolve(
        &self,
        src: &Source,
        majors: &[String],
        lock: &mut Lock,
    ) -> Result<Resolved, String> {
        one_namespace(src)?;
        let key = src.to_string();
        if let Some(pin) = lock.modules.get(&key).cloned() {
            // No protocol recorded (Go omitted it when empty) is a Go-era pin too: resolved again.
            if pin.protocol >= self.protocol {
                return self.pinned(src, majors, &pin);
            }
            let mut r = self.fresh(src, majors, lock)?;
            r.notes.push(format!(
                "{key} {} (pinned in doze.lock) speaks module protocol {}; this doze speaks {}, so it now runs {}",
                pin.version, pin.protocol, self.protocol, r.version
            ));
            return Ok(r);
        }
        self.fresh(src, majors, lock)
    }

    /// `doze modules upgrade`: resolves `src` again ignoring its pin, and pins what it took. The version it was pinned
    /// to before, if any, and the new one.
    pub fn upgrade(
        &self,
        src: &Source,
        majors: &[String],
        lock: &mut Lock,
    ) -> Result<(Option<String>, Resolved), String> {
        one_namespace(src)?;
        let key = src.to_string();
        let old = lock.modules.get(&key).map(|p| p.version.clone());
        let mut fresh = Lock::default();
        let r = self.fresh(src, majors, &mut fresh)?;
        if let Some(pin) = fresh.modules.remove(&key) {
            lock.record_module(&key, pin);
        }
        Ok((old, r))
    }

    fn pinned(&self, src: &Source, majors: &[String], pin: &Pin) -> Result<Resolved, String> {
        let key = src.to_string();
        let doze = self.doze.trim_start_matches('v');
        if select::is_doze_version(&self.doze)
            && !pin.doze.is_empty()
            && select::compare_versions(&pin.doze, doze).is_gt()
        {
            return Err(format!(
                "module {key} {} (pinned in doze.lock) needs doze {} or newer; this doze is {doze}. Update doze (in the app: Settings › About); the lock was written by a newer doze",
                pin.version, pin.doze
            ));
        }
        if pin.protocol > self.protocol {
            return Err(format!(
                "module {key} {} (pinned in doze.lock) speaks module protocol {}; this doze speaks {}. Update doze",
                pin.version, pin.protocol, self.protocol
            ));
        }
        if let Some(major) = majors
            .iter()
            .find(|m| !pin.engines.is_empty() && !pin.engines.contains(m))
        {
            return Err(format!(
                "{} {major} needs a newer {key} module: pinned {} supports {} — run 'doze modules upgrade {}'",
                src.name(),
                pin.version,
                pin.engines.join(", "),
                src.name()
            ));
        }
        let expected = pin.hashes.get(&self.triple).map(String::as_str);
        let content = self.content_dir(src, &pin.version);
        if crate::archive::has_files(&content.join("bin")) {
            let what = format!("module {key} {}", pin.version);
            let (bin, _) = mirror::cached(&content, &what, expected)?;
            return self.ready(src, &pin.version, &bin, Vec::new());
        }
        let idx = self.index(src)?;
        let rel = idx.releases.get(&pin.version).ok_or_else(|| {
            format!(
                "module {key} {} (pinned in doze.lock) is no longer published — run 'doze modules upgrade {}'",
                pin.version,
                src.name()
            )
        })?;
        let bin = self.download(src, &pin.version, rel, expected)?;
        self.ready(src, &pin.version, &bin, Vec::new())
    }

    fn fresh(&self, src: &Source, majors: &[String], lock: &mut Lock) -> Result<Resolved, String> {
        let key = src.to_string();
        let idx = match self.index(src) {
            Ok(idx) => idx,
            Err(err) => {
                let Some((v, bin)) = self.newest_cached(src) else {
                    return Err(err);
                };
                let note = format!(
                    "the registry can't be reached ({err}); using {key} {v} from this machine's cache, not pinned"
                );
                return self.ready(src, &v, &bin, vec![note]);
            }
        };
        let version = match select::newest_that_fits(&idx, self.here(), majors) {
            Fit::Release(v) if crate::lock::plain(&v) => v,
            Fit::Release(v) => return Err(format!("module {key}: {v:?} is not a plain version")),
            other => return Err(self.no_fit(src, majors, &other)),
        };
        let rel = idx
            .releases
            .get(&version)
            .ok_or_else(|| format!("module {key} {version} is not in its index"))?;
        let bin = self.download(src, &version, rel, None)?;
        let pin = Pin {
            version: version.clone(),
            protocol: rel.protocol,
            engines: rel.engines.clone(),
            hashes: rel
                .artifacts
                .iter()
                .map(|(t, a)| {
                    (
                        t.clone(),
                        format!("sha256:{}", a.sha256.to_ascii_lowercase()),
                    )
                })
                .collect(),
            doze: rel.doze.clone(),
        };
        let mut notes = Vec::new();
        if !lock.modules.contains_key(&key) {
            notes.push(format!(
                "pinned {key} {version} in doze.lock — commit the lockfile so your team and CI get this exact build"
            ));
        }
        lock.record_module(&key, pin);
        self.ready(src, &version, &bin, notes)
    }

    fn no_fit(&self, src: &Source, majors: &[String], fit: &Fit) -> String {
        let what = if majors.is_empty() {
            src.short()
        } else {
            format!("{} {}", src.short(), majors.join(", "))
        };
        match fit {
            Fit::NeedsDoze(need) => format!(
                "{what} needs doze {need} or newer; this doze is {}. Update doze (in the app: Settings › About), then run the command again",
                self.doze.trim_start_matches('v')
            ),
            Fit::Protocol { min, max } if *max > self.protocol => format!(
                "module {src} now speaks module protocol {max}; this doze speaks {}. Update doze (in the app: Settings › About)",
                self.protocol
            ),
            Fit::Protocol { min, max } => format!(
                "no release of module {src} speaks module protocol {} (its releases speak {min}–{max}); it has to be published for this doze",
                self.protocol
            ),
            Fit::Engines => format!("no release of module {src} runs {what}"),
            Fit::Empty | Fit::Release(_) => format!("module {src} has no releases"),
        }
    }

    /// The module's signed index, verified with the registry key.
    pub fn index(&self, src: &Source) -> Result<Index, String> {
        let path = src.file("index.yaml");
        let got = self.registry.get(&path).map_err(|e| match e {
            FetchError::Missing { from } => {
                format!("module {src} is not published in the registry ({from} serves no index)")
            }
            other => format!("fetching module index {path}: {}", other.reasons()),
        })?;
        let text = String::from_utf8_lossy(&got.body);
        let idx = index::parse(&text).map_err(|e| format!("{path}: {}", e.message))?;
        if idx.module != src.name() {
            return Err(format!(
                "{path}: index is for module {:?}, expected {:?}",
                idx.module,
                src.name()
            ));
        }
        index::verify(&idx, &self.registry.key)
            .map_err(|r| format!("module index for {src}: {r}"))?;
        Ok(idx)
    }

    /// Release `version` of `src` for this platform, from the verified index, downloaded and checked (against
    /// `expected`, a lock's `sha256:<hex>`, when given) into the cache; its `bin/`. `modules verify` and the live
    /// registry test use it.
    pub fn fetch(
        &self,
        src: &Source,
        version: &str,
        expected: Option<&str>,
    ) -> Result<PathBuf, String> {
        let idx = self.index(src)?;
        let rel = idx
            .releases
            .get(version)
            .ok_or_else(|| format!("module {src} {version} is not published"))?;
        self.download(src, version, rel, expected)
    }

    /// Downloads release `version`'s archive for this platform, checked against the lock's pin (else the index's
    /// checksum) and the registry's signature, into the cache.
    fn download(
        &self,
        src: &Source,
        version: &str,
        rel: &Release,
        expected: Option<&str>,
    ) -> Result<PathBuf, String> {
        let art = rel
            .artifacts
            .get(&self.triple)
            .ok_or_else(|| format!("module {src} {version} has no artifact for {}", self.triple))?;
        let url = self.artifact_url(src, &art.url);
        let want = Want {
            manifest_sha: &art.sha256,
            sig: &art.sig,
            expected,
            key: Some(&self.registry.key),
        };
        let what = format!("module {src} {version} ({})", self.triple);
        let content = self.content_dir(src, version);
        let (bin, _) = mirror::download(&url, &content, &what, &want)?;
        // The verified release's protocol, beside the build: the offline fallback takes only builds this doze speaks
        // (the Go doze shares this cache, and its builds speak protocol 1).
        std::fs::write(content.join(PROTOCOL_FILE), format!("{}\n", rel.protocol))
            .map_err(|e| format!("recording {what}: {e}"))?;
        Ok(bin)
    }

    /// An artifact's address: absolute as published, or relative to the module's folder on the first registry base.
    fn artifact_url(&self, src: &Source, url: &str) -> String {
        if url.contains("://") {
            return url.to_string();
        }
        let rel = src.file(url.trim_start_matches('/'));
        match self.registry.bases.first() {
            Some(registry::Base::Http(b)) => format!("{}/{rel}", b.trim_end_matches('/')),
            Some(registry::Base::Dir(d)) => format!("file://{}", d.join(rel).display()),
            None => rel,
        }
    }

    /// The newest build of `src` this machine has for this platform.
    fn newest_cached(&self, src: &Source) -> Option<(String, PathBuf)> {
        let dir = self.home.join("modules").join(src.ns()).join(src.name());
        let suffix = format!("-{}", self.triple);
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let v = name.strip_suffix(&suffix)?.to_string();
                let bin = e.path().join("bin");
                let speaks = std::fs::read_to_string(e.path().join(PROTOCOL_FILE))
                    .is_ok_and(|p| p.trim() == self.protocol.to_string());
                (speaks && crate::archive::has_files(&bin)).then_some((v, bin))
            })
            .max_by(|a, b| select::compare_versions(&a.0, &b.0))
    }

    fn ready(
        &self,
        src: &Source,
        version: &str,
        bin: &Path,
        notes: Vec<String>,
    ) -> Result<Resolved, String> {
        let exe = plugin_exe(bin, src.name())
            .ok_or_else(|| format!("module {src} {version} has no plugin executable"))?;
        Ok(Resolved {
            exe,
            version: version.into(),
            notes,
        })
    }
}

/// Modules come from one registry, all under its `doze` namespace (Go's `nsManager`).
fn one_namespace(src: &Source) -> Result<(), String> {
    let ns = crate::registry::source::DEFAULT_NAMESPACE;
    if src.ns() == ns {
        Ok(())
    } else {
        Err(format!(
            "module namespace {:?}: doze has one registry, and its modules are all under {ns:?}",
            src.ns()
        ))
    }
}

/// The plugin in `bin`: `<name>-plugin` by convention (with `.exe` on Windows), else the first file there.
pub fn plugin_exe(bin: &Path, name: &str) -> Option<PathBuf> {
    for file in [format!("{name}-plugin"), format!("{name}-plugin.exe")] {
        let p = bin.join(file);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(bin)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files.into_iter().next()
}

#[cfg(test)]
#[path = "modules_tests.rs"]
mod tests;
