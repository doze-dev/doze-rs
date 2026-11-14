//! `doze.lock`, beside `doze.hcl`: what a project pinned, read and written in the Go doze's format (yaml.v3 output of
//! Go's `binaries.Lock`), so the file stays the same when a project moves between the two and diffs only what changed.
//!
//! - `engines:` engine builds by version spec: the full version, where it came from, each platform's archive hash.
//! - `modules:` each module source pinned to one release, with its protocol, engine majors, hashes and oldest doze.
//! - `downloads:` each fetched URL (a `service` block's `download`) pinned to the checksum it first served.

use crate::registry::select::compare_versions;
use crate::registry::yaml::{self, YamlError, entries, get, scalar, sub, text, texts};
use crate::yaml_out::{key_order, scalar as out};
use std::collections::BTreeMap;
use std::path::Path;
use yaml_rust2::Yaml;

/// The file's name, beside `doze.hcl`.
pub const FILE: &str = "doze.lock";

/// One module pin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pin {
    pub version: String,
    pub protocol: i64,
    pub engines: Vec<String>,
    /// Platform triple to `sha256:<hex>`.
    pub hashes: BTreeMap<String, String>,
    pub doze: String,
}

/// One engine build pin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnginePin {
    /// The full version (`16.4.0`).
    pub resolved: String,
    /// `mirror`, `override`.
    pub source: String,
    pub hashes: BTreeMap<String, String>,
}

/// A whole lock.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lock {
    /// The newest doze that wrote it.
    pub doze: String,
    /// Engine, then version spec, to its pin.
    pub engines: BTreeMap<String, BTreeMap<String, EnginePin>>,
    pub modules: BTreeMap<String, Pin>,
    /// URL to `sha256:<hex>`.
    pub downloads: BTreeMap<String, String>,
    /// Something changed since it was read (or a versionless pin was dropped while reading, as Go does).
    pub dirty: bool,
}

fn hashes(h: &yaml_rust2::yaml::Hash) -> Result<BTreeMap<String, String>, YamlError> {
    Ok(sub(h, "hashes")?
        .into_iter()
        .flat_map(entries)
        .filter_map(|(t, v)| scalar(v).map(|v| (t, v)))
        .collect())
}

// fuzz:parser fuzz_lock
/// Reads a lock. Module pins without a version (written before pins had one) are left out, as Go drops them.
/// Whether `v` is a plain version or name, safe as one path component: letters, digits, `.`, `+`, `-`, `_`; not
/// empty, not starting with `.`, no `..`. A lock (or a module's request) that names anything else is refused: the
/// version becomes a cache folder's name, and `/abs` or `../` would put a build outside the cache.
pub fn plain(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && !v.starts_with('.')
        && !v.contains("..")
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-' | b'_'))
}

fn refuse(what: &str, v: &str) -> YamlError {
    YamlError::new(format!("{what} {v:?} is not a plain version"))
}

pub fn parse(src: &str) -> Result<Lock, YamlError> {
    if src.trim().is_empty() {
        return Ok(Lock::default());
    }
    if let Some(at) = conflict(src) {
        return Err(YamlError {
            message: "a merge conflict was left in it".into(),
            at: Some(at),
        });
    }
    let h = yaml::mapping(src)?;
    let mut lock = Lock {
        doze: text(&h, "doze"),
        ..Lock::default()
    };
    for (source, pin) in sub(&h, "modules")?.into_iter().flat_map(entries) {
        let Yaml::Hash(p) = pin else { continue };
        let version = text(p, "version");
        if version.is_empty() {
            lock.dirty = true;
            continue;
        }
        if !plain(&version) {
            return Err(refuse(&format!("the pin of {source}:"), &version));
        }
        let pin = Pin {
            version,
            protocol: get(p, "protocol").as_i64().unwrap_or(0),
            engines: texts(p, "engines"),
            hashes: hashes(p)?,
            doze: text(p, "doze"),
        };
        lock.modules.insert(source, pin);
    }
    for (engine, specs) in sub(&h, "engines")?.into_iter().flat_map(entries) {
        let Yaml::Hash(specs) = specs else { continue };
        let mut by_spec = BTreeMap::new();
        for (spec, p) in entries(specs) {
            let Yaml::Hash(p) = p else { continue };
            let resolved = text(p, "resolved");
            if !resolved.is_empty() && !plain(&resolved) {
                return Err(refuse(&format!("the {engine} pin:"), &resolved));
            }
            let pin = EnginePin {
                resolved,
                source: text(p, "source"),
                hashes: hashes(p)?,
            };
            by_spec.insert(spec, pin);
        }
        lock.engines.insert(engine, by_spec);
    }
    for (url, p) in sub(&h, "downloads")?.into_iter().flat_map(entries) {
        if let Yaml::Hash(p) = p {
            lock.downloads.insert(url, text(p, "sha256"));
        }
    }
    Ok(lock)
}

/// Where a git conflict marker starts, if the file has one.
fn conflict(src: &str) -> Option<usize> {
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        if line.starts_with("<<<<<<< ")
            || line.starts_with("=======")
            || line.starts_with(">>>>>>> ")
        {
            return Some(at);
        }
        at += line.len();
    }
    None
}

impl Lock {
    /// Reads `path`; a missing file is an empty lock.
    pub fn load(path: &Path) -> Result<Lock, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                parse(&text).map_err(|e| format!("parsing {}: {}", path.display(), e.message))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lock::default()),
            Err(e) => Err(format!("reading {}: {e}", path.display())),
        }
    }

    /// Pins `source` to `pin` (Go's `RecordModule`); a pin without a version is ignored.
    pub fn record_module(&mut self, source: &str, pin: Pin) {
        if pin.version.is_empty() || self.modules.get(source) == Some(&pin) {
            return;
        }
        self.modules.insert(source.into(), pin);
        self.dirty = true;
    }

    /// Forgets the pin of `source`.
    pub fn drop_module(&mut self, source: &str) {
        self.dirty |= self.modules.remove(source).is_some();
    }

    /// Pins an engine build (Go's `Record`): a new resolved version replaces the pin; hashes are added to.
    pub fn record_engine(&mut self, engine: &str, spec: &str, pin: EnginePin) {
        let specs = self.engines.entry(engine.into()).or_default();
        let had = specs.get(spec).cloned();
        let p = specs.entry(spec.into()).or_default();
        if had.as_ref().is_none_or(|h| h.resolved != pin.resolved) {
            *p = EnginePin {
                resolved: pin.resolved.clone(),
                source: pin.source.clone(),
                hashes: BTreeMap::new(),
            };
            self.dirty = true;
        }
        if p.source.is_empty() && !pin.source.is_empty() {
            p.source = pin.source;
            self.dirty = true;
        }
        for (triple, sha) in pin.hashes {
            if p.hashes.get(&triple) != Some(&sha) {
                p.hashes.insert(triple, sha);
                self.dirty = true;
            }
        }
    }

    /// Pins a download to the checksum it served first; an existing pin is kept (Go's `RecordDownload`).
    pub fn record_download(&mut self, url: &str, digest: &str) {
        if digest.is_empty() || self.downloads.get(url).is_some_and(|d| !d.is_empty()) {
            return;
        }
        self.downloads.insert(url.into(), digest.into());
        self.dirty = true;
    }

    /// Writes the lock if it changed. `doze` is this doze's version when it is a release: it becomes the lock's
    /// `doze:` when that is empty or older (Go's `Save`). Written whole or not at all.
    pub fn save(&mut self, path: &Path, doze: Option<&str>) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(v) = doze
            .filter(|v| crate::registry::select::is_doze_version(v))
            .map(|v| v.trim_start_matches('v'))
            && (self.doze.is_empty() || compare_versions(&self.doze, v).is_lt())
        {
            self.doze = v.into();
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("lock.tmp{}", std::process::id()));
        std::fs::write(&tmp, render(self))?;
        std::fs::rename(&tmp, path)?;
        self.dirty = false;
        Ok(())
    }
}

/// The file's canonical path: its folder resolved (symlinks, `..`), its own name kept, so a lock reached two ways
/// is one file (the file itself may not exist yet).
pub fn canonical(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().map(std::ffi::OsStr::to_os_string);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match (dir.canonicalize(), name) {
        (Ok(d), Some(n)) => d.join(n),
        _ => path.to_path_buf(),
    }
}

/// Changes the lock at `path` with `change` and saves it, holding a lock in the doze home keyed by the file's
/// canonical path (never a file beside the project's), so every writer (`doze modules upgrade`, each `__fetch`
/// child) reads the file again under it and no one's pin is lost. `doze` is as for [`Lock::save`].
pub fn update<T>(
    home: &Path,
    path: &Path,
    doze: Option<&str>,
    change: impl FnOnce(&mut Lock) -> Result<T, String>,
) -> Result<T, String> {
    let locks = home.join("locks");
    std::fs::create_dir_all(&locks)
        .map_err(|e| format!("taking the doze.lock lock in {}: {e}", locks.display()))?;
    let key = crate::registry::signed::sha256_hex(canonical(path).to_string_lossy().as_bytes());
    let _held =
        doze_os::fs::lock(&locks.join(format!("lock-{}.lock", key.get(..16).unwrap_or(&key))))
            .map_err(|e| e.to_string())?;
    let mut now = Lock::load(path)?;
    let out = change(&mut now)?;
    now.save(path, doze)
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(out)
}

fn sorted<V>(m: &BTreeMap<String, V>) -> Vec<(&String, &V)> {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| key_order(a.0, b.0));
    v
}

fn map_block(o: &mut String, indent: usize, key: &str, m: &BTreeMap<String, String>) {
    if m.is_empty() {
        return;
    }
    let pad = " ".repeat(indent);
    o.push_str(&format!("{pad}{key}:\n"));
    for (k, v) in sorted(m) {
        o.push_str(&format!("{pad}    {}: {}\n", out(k), out(v)));
    }
}

/// The lock as Go's yaml.v3 `Marshal` writes it.
pub fn render(lock: &Lock) -> String {
    let mut o = String::new();
    if !lock.doze.is_empty() {
        o.push_str(&format!("doze: {}\n", out(&lock.doze)));
    }
    if lock.engines.is_empty() {
        o.push_str("engines: {}\n");
    } else {
        o.push_str("engines:\n");
        for (engine, specs) in sorted(&lock.engines) {
            o.push_str(&format!("    {}:", out(engine)));
            o.push_str(if specs.is_empty() { " {}\n" } else { "\n" });
            for (spec, p) in sorted(specs) {
                o.push_str(&format!("        {}:\n", out(spec)));
                o.push_str(&format!("            resolved: {}\n", out(&p.resolved)));
                o.push_str(&format!("            source: {}\n", out(&p.source)));
                map_block(&mut o, 12, "hashes", &p.hashes);
            }
        }
    }
    render_modules(&mut o, lock);
    if !lock.downloads.is_empty() {
        o.push_str("downloads:\n");
        for (url, sha) in sorted(&lock.downloads) {
            o.push_str(&format!(
                "    {}:\n        sha256: {}\n",
                out(url),
                out(sha)
            ));
        }
    }
    o
}

fn render_modules(o: &mut String, lock: &Lock) {
    if lock.modules.is_empty() {
        return;
    }
    o.push_str("modules:\n");
    for (source, p) in sorted(&lock.modules) {
        o.push_str(&format!("    {}:\n", out(source)));
        o.push_str(&format!("        version: {}\n", out(&p.version)));
        if p.protocol != 0 {
            o.push_str(&format!("        protocol: {}\n", p.protocol));
        }
        if !p.engines.is_empty() {
            o.push_str("        engines:\n");
            for e in &p.engines {
                o.push_str(&format!("            - {}\n", out(e)));
            }
        }
        map_block(o, 8, "hashes", &p.hashes);
        if !p.doze.is_empty() {
            o.push_str(&format!("        doze: {}\n", out(&p.doze)));
        }
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
