//! The record each stack keeps between runs (`<home>/projects/<slug>/project.json`), read as Go's `projects.Load`
//! and `projects.List` read it.

use crate::gojson::{self, is};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// The record's file name inside a project's state folder.
pub const FILE_NAME: &str = "project.json";

/// The record version this reader knows. A newer one is read all the same: its unknown fields are kept in `extra`.
pub const SCHEMA: i64 = 1;

/// What is remembered about one stack.
#[derive(Debug, Clone, Default)]
pub struct Project {
    /// The record's version.
    pub schema: i64,
    /// The absolute path of its doze.hcl.
    pub config: String,
    /// The stack's name; empty when none was recorded.
    pub name: String,
    /// When `doze up` last ran for it.
    pub last_up: Option<OffsetDateTime>,
    /// When `doze down` last ran for it.
    pub last_down: Option<OffsetDateTime>,
    /// What was started last time, as the names asked for (their dependencies follow). Empty means everything.
    pub last_set: Vec<String>,
    /// Fields a newer doze wrote, as written (Go keeps them through a save; this reader only shows them).
    pub extra: BTreeMap<String, Box<RawValue>>,
    dir: PathBuf,
}

impl Project {
    /// The project's state folder, which is also its stack key.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// When it was last used: the later of its last up and last down.
    pub fn last_used(&self) -> Option<OffsetDateTime> {
        if self.last_down > self.last_up {
            self.last_down
        } else {
            self.last_up
        }
    }
}

/// Why a record could not be read.
#[derive(Debug)]
pub enum Error {
    /// The file could not be read.
    Io(io::Error),
    /// The file is not a record Go would load.
    Decode(PathBuf, serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Decode(path, e) => write!(f, "{}: {e}", path.display()),
        }
    }
}

impl std::error::Error for Error {}

/// Reads the record in `dir`. A project never recorded is `Ok(None)`.
pub fn load(dir: &Path) -> Result<Option<Project>, Error> {
    let path = dir.join(FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    };
    let mut p = decode(&String::from_utf8_lossy(&bytes)).map_err(|e| Error::Decode(path, e))?;
    p.dir = dir.to_path_buf();
    Ok(Some(p))
}

/// The keys Go's record knows, matched exactly as Go matches them when it sets the rest aside.
const KNOWN: [&str; 6] = [
    "schema",
    "config",
    "name",
    "last_up",
    "last_down",
    "last_set",
];

/// Decodes a record's text as Go's `json.Unmarshal` does, keeping the keys it does not know.
pub fn decode(text: &str) -> serde_json::Result<Project> {
    let mut p = Project::default();
    let text = gojson::mend_surrogates(text);
    for (key, raw) in gojson::object(&text)?.unwrap_or_default() {
        field(&key, raw, &mut p)?;
        if !KNOWN.contains(&key.as_str()) {
            p.extra.insert(key, raw.to_owned());
        }
    }
    Ok(p)
}

fn field(key: &str, raw: &RawValue, p: &mut Project) -> serde_json::Result<()> {
    if is(key, "schema") {
        set(&mut p.schema, gojson::value(raw)?);
    } else if is(key, "config") {
        set(&mut p.config, gojson::value(raw)?);
    } else if is(key, "name") {
        set(&mut p.name, gojson::value(raw)?);
    } else if is(key, "last_up") {
        set(&mut p.last_up, gojson::time(raw)?.map(Some));
    } else if is(key, "last_down") {
        set(&mut p.last_down, gojson::time(raw)?.map(Some));
    } else if is(key, "last_set") {
        p.last_set = gojson::strings(raw)?;
    }
    Ok(())
}

fn set<T>(slot: &mut T, v: Option<T>) {
    if let Some(v) = v {
        *slot = v;
    }
}

/// Every recorded project under `home`, most recently used first (ties keep folder-name order). A record that cannot
/// be read is skipped: one bad file must not hide the rest. A home with no projects folder lists none.
pub fn list(home: &Path) -> Vec<Project> {
    let mut out: Vec<Project> = crate::paths::project_dirs(home)
        .into_iter()
        .filter(|d| d.join(FILE_NAME).symlink_metadata().is_ok())
        .filter_map(|d| load(&d).ok().flatten())
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.last_used()));
    out
}
