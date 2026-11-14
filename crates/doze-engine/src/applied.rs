//! Applied state (Go's `internal/state`): per project, the structural objects doze has applied for each instance (roles,
//! databases, buckets, queues), the outputs it last rendered, and how each task last ran, in `.doze/state.json` beside
//! the config, in Go's JSON so either doze reads the other's. `doze sync` diffs the declared objects against it:
//! creates what is new, updates what changed, prunes what is no longer declared.

use doze_module_proto::msg::instance::Object;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// The on-disk shape's version.
const VERSION: u32 = 1;

/// How a task last ran: what `doze task` and the app show, and what decides whether it runs again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    /// `done` or `failed`.
    pub result: String,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default)]
    pub took_ms: i64,
    /// When it started, RFC 3339; empty is written as Go's zero time, which Go reads (an empty string it refuses, and
    /// with it the whole file: E2 audit 1, m3).
    #[serde(default, serialize_with = "go_time")]
    pub ran_at: String,
    /// What made it run.
    #[serde(default)]
    pub why: String,
    /// The hash of its inputs when it started.
    #[serde(default)]
    pub inputs: String,
}

fn go_time<S: serde::Serializer>(t: &str, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(if t.is_empty() {
        "0001-01-01T00:00:00Z"
    } else {
        t
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ObjectJson {
    kind: String,
    name: String,
    hash: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Disk {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    instances: BTreeMap<String, Vec<ObjectJson>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    outputs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tasks: BTreeMap<String, TaskRecord>,
}

/// The applied-object record of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// The objects applied for each instance, in the order they were applied.
    pub instances: BTreeMap<String, Vec<Object>>,
    /// The last-applied output values.
    pub outputs: BTreeMap<String, String>,
    /// How each task last ran.
    pub tasks: BTreeMap<String, TaskRecord>,
}

/// The state file for a config: `.doze/state.json` beside it (in it, when the path is a folder).
pub fn path_for(config: &Path) -> PathBuf {
    let dir = if config.is_dir() {
        config.to_path_buf()
    } else {
        config
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    };
    dir.join(".doze").join("state.json")
}

impl Applied {
    /// Reads the state at `path`; a missing file is an empty state.
    pub fn load(path: &Path) -> io::Result<Applied> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Applied::default()),
            Err(e) => return Err(e),
        };
        let disk: Disk = serde_json::from_slice(&bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("parsing {}: {e}", path.display()),
            )
        })?;
        let to_obj = |o: ObjectJson| Object {
            kind: o.kind,
            name: o.name,
            hash: o.hash,
        };
        Ok(Applied {
            instances: disk
                .instances
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().map(to_obj).collect()))
                .collect(),
            outputs: disk.outputs,
            tasks: disk.tasks,
        })
    }

    /// Writes it to `path` whole (a temp file renamed over it), making `.doze/` as Go does.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let to_json = |o: &Object| ObjectJson {
            kind: o.kind.clone(),
            name: o.name.clone(),
            hash: o.hash.clone(),
        };
        let disk = Disk {
            version: VERSION,
            instances: self
                .instances
                .iter()
                .map(|(k, v)| (k.clone(), v.iter().map(to_json).collect()))
                .collect(),
            outputs: self.outputs.clone(),
            tasks: self.tasks.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&disk).map_err(io::Error::other)?;
        if let Some(dir) = path.parent() {
            ensure_dot_doze(dir)?;
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }

    /// The objects applied for `instance`.
    pub fn objects(&self, instance: &str) -> &[Object] {
        self.instances.get(instance).map_or(&[], Vec::as_slice)
    }

    /// Records the objects applied for `instance`, replacing the last set; none clears it.
    pub fn set(&mut self, instance: &str, objs: Vec<Object>) {
        if objs.is_empty() {
            self.instances.remove(instance);
        } else {
            self.instances.insert(instance.into(), objs);
        }
    }
}

/// Makes `dir`; a `.doze` folder also gets a `.gitignore` that ignores everything in it (Go's `dotdoze.Ensure`), once.
pub fn ensure_dot_doze(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    if dir.file_name().is_none_or(|n| n != ".doze") {
        return Ok(());
    }
    let ignore = "# Written by doze: this folder is this machine's state, so nothing in it is committed.\n*\n";
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(".gitignore"))
    {
        Ok(mut f) => std::io::Write::write_all(&mut f, ignore.as_bytes()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// Holds the state file's lock while it lives: one `sync` at a time per project. The lock is the system's (released
/// however the holder ends), so no stale lock is left to break (Go's was a file broken after 2 minutes).
#[derive(Debug)]
pub struct Locked(#[allow(dead_code)] doze_os::fs::Lock);

/// Takes the lock for the state at `path`, or says another sync holds it. It waits up to a second first: a process
/// being spawned meanwhile holds a copy of the lock's descriptor until it execs, which would otherwise look like
/// another sync (E2 audit 1, M3).
pub fn lock(path: &Path) -> io::Result<Locked> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match try_lock_once(path) {
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            other => return other,
        }
    }
}

fn try_lock_once(path: &Path) -> io::Result<Locked> {
    let mut name = path.as_os_str().to_owned();
    name.push(".lock");
    let lock_path = PathBuf::from(name);
    if let Some(dir) = lock_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match doze_os::fs::try_lock(&lock_path)? {
        Some(l) => Ok(Locked(l)),
        None => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "another doze sync is in progress (it holds {})",
                lock_path.display()
            ),
        )),
    }
}

/// What a sync does to one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Create,
    Update,
    Delete,
}

impl Change {
    /// The plan's symbol: `+`, `~`, `-`.
    pub fn symbol(self) -> &'static str {
        match self {
            Change::Create => "+",
            Change::Update => "~",
            Change::Delete => "-",
        }
    }
}

fn same(a: &Object, b: &Object) -> bool {
    a.kind == b.kind && a.name == b.name
}

/// The changes from `prior` to `desired`: creates (new), updates (another hash), in desired order; then deletes (no
/// longer declared) in reverse prior order, so dependents drop before what they depend on.
pub fn diff(prior: &[Object], desired: &[Object]) -> Vec<(Change, Object)> {
    let mut out = Vec::new();
    for o in desired {
        match prior.iter().find(|p| same(p, o)) {
            None => out.push((Change::Create, o.clone())),
            Some(p) if p.hash != o.hash => out.push((Change::Update, o.clone())),
            Some(_) => {}
        }
    }
    out.extend(
        removed(prior, desired)
            .into_iter()
            .map(|o| (Change::Delete, o)),
    );
    out
}

/// The objects in `prior` and not in `desired`, in reverse prior order: what a sync prunes.
pub fn removed(prior: &[Object], desired: &[Object]) -> Vec<Object> {
    prior
        .iter()
        .rev()
        .filter(|p| !desired.iter().any(|o| same(p, o)))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "applied_tests.rs"]
mod tests;
