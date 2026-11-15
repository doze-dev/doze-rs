//! Where a stack's state lives, as the Go doze lays it out: `<home>/projects/<slug>/`, its runtime files in `run/`.
//! A stack is keyed by that folder, as the Go app keys it.

use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// A stack's key: its state folder, `<home>/projects/<slug>`. The Go app keys stacks the same way, so a key names one
/// stack whether it is running (found by its socket) or stopped (found by its record).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StackKey(PathBuf);

impl StackKey {
    /// The key of the state folder `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self(dir.into())
    }

    /// The key of the stack whose control socket is `sock` (`<key>/run/doze.admin.sock`).
    pub fn from_socket(sock: &Path) -> Option<Self> {
        sock.parent()?.parent().map(Self::new)
    }

    /// The key of the stack whose doze.hcl is at `config`, under `home`.
    pub fn for_config(home: &Path, config: &Path) -> Self {
        Self(home.join("projects").join(slug(config)))
    }

    /// The state folder.
    pub fn dir(&self) -> &Path {
        &self.0
    }

    /// The folder's slug: `<base>-<hash>`.
    pub fn slug(&self) -> String {
        self.0
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The runtime folder: sockets, pid file, log, journal.
    pub fn run_dir(&self) -> PathBuf {
        self.0.join("run")
    }

    /// The daemon's control socket.
    pub fn socket(&self) -> PathBuf {
        self.run_dir().join("doze.admin.sock")
    }

    /// The daemon's pid file.
    pub fn pid_file(&self) -> PathBuf {
        self.run_dir().join("doze.pid")
    }

    /// The daemon's log.
    pub fn log_file(&self) -> PathBuf {
        self.run_dir().join("doze.log")
    }

    /// The daemon's journal (read with doze-journal).
    pub fn journal(&self) -> PathBuf {
        self.run_dir().join("journal.jsonl")
    }

    /// The project record.
    pub fn record(&self) -> PathBuf {
        self.0.join(crate::FILE_NAME)
    }
}

/// The slug the Go doze gives the project whose doze.hcl is at `config`: the base name of the folder holding it,
/// lowercased with every run of other characters made one dash, then a dash and the first 8 hex digits of the
/// SHA-256 of that folder's absolute path. So two projects with one base name never share a folder.
pub fn slug(config: &Path) -> String {
    if config.as_os_str().is_empty() {
        return "default".to_owned();
    }
    let abs = absolute(config);
    let root = abs.parent().unwrap_or(&abs);
    let hash = hex::encode(Sha256::digest(root.to_string_lossy().as_bytes()));
    let base = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}-{}", sanitize(&base), hash.get(..8).unwrap_or(&hash))
}

/// Go's `filepath.Abs`: joined to the working directory if relative, then cleaned lexically (no symlinks resolved).
fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn sanitize(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in s.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() { "project" } else { out }.to_owned()
}

/// Every folder under `<home>/projects`, in name order (Go's glob order). None when there is no such folder.
pub(crate) fn project_dirs(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home.join("projects")) else {
        return Vec::new();
    };
    let mut names: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| home.join("projects").join(n))
        .collect()
}
