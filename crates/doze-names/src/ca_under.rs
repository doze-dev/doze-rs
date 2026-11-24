//! Leaves for names under a registered one (`abc.execute-api.us-east-1.files.shop.doze`, which doze-aws mints at run
//! time): a leaf for exactly that name, kept under `certs/under/`.
//!
//! **Bounded.** Nothing bounds how many such names a program makes, so at most [`UNDER_CAP`] are kept in memory (the
//! least recently served goes) and on disk (the oldest file goes). The files on disk are counted once and then kept
//! count of, so the folder is listed only when a new leaf takes it past the cap.
//!
//! **Concurrent.** Each name has a slot of its own. The shared map is locked only to find the slot; loading or minting
//! happens under the slot's lock, so the same name is minted once however many handshakes ask at once, while other
//! names go on in parallel. The front door calls this off its async workers (`front_door.rs`).
//!
//! **Files.** Named by a readable prefix of the host and a hash of all of it, so a 253-character host still makes a
//! short file name. The subject is `O=doze` with no common name (a function URL's host is past X.520's 64 characters);
//! the SAN carries the name.

use crate::Error;
use crate::ca::{CERTS, Ca, Leaf, UNDER, ca_err};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How many leaves for names under a registered one are kept, in memory and on disk. A person works with a handful of
/// APIs and function URLs at a time, and a leaf costs about 1.5 KB and a millisecond to mint, so 256 keeps every
/// working set warm while a program that mints a fresh API id per test run leaves at most ~400 KB behind.
pub const UNDER_CAP: usize = 256;

/// A temporary file older than this was left by a writer that died: removed when the folder is pruned.
const STRAY_AFTER: Duration = Duration::from_secs(60);

/// One name's leaf once there is one, locked by whoever loads or mints it.
pub(crate) type Slot = Arc<Mutex<Option<Arc<Leaf>>>>;

/// The slots by exact name, each with when it was last asked for, and the files on disk as last counted.
#[derive(Debug, Default)]
pub(crate) struct Under {
    slots: HashMap<String, (Slot, u64)>,
    tick: u64,
    on_disk: Option<usize>,
}

impl Under {
    /// The slot for `name`, made when there is none; past the cap the least recently asked for goes (a mint in flight
    /// in it finishes, uncached).
    pub(crate) fn slot(&mut self, name: &str) -> Slot {
        self.tick += 1;
        let tick = self.tick;
        if let Some((s, used)) = self.slots.get_mut(name) {
            *used = tick;
            return s.clone();
        }
        if self.slots.len() >= UNDER_CAP {
            let oldest = self
                .slots
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(n, _)| n.clone());
            if let Some(n) = oldest {
                self.slots.remove(&n);
            }
        }
        let s = Slot::default();
        self.slots.insert(name.to_string(), (s.clone(), tick));
        s
    }
}

/// Whether `name` is a host name: DNS letters, digits, hyphens (and underscores), no empty label, at most 253 long.
fn plain_host(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        })
}

/// The file name (without extension) a name's leaf is kept under: up to 64 characters of it, then a hash of all of it.
pub(crate) fn file_stem(name: &str) -> String {
    let prefix: String = name.chars().take(64).collect();
    let hash = hex::encode(Sha256::digest(name.as_bytes()));
    format!(
        "{}-{}",
        prefix.trim_end_matches('.'),
        hash.get(..16).unwrap_or(&hash)
    )
}

fn poisoned<T>(_: T) -> Error {
    ca_err("the leaf cache lock is poisoned")
}

impl Ca {
    /// The certificate for exactly `name`, a name under a registered one: from memory, from `certs/under/`, or minted
    /// now; the caller has checked that a registered name owns it. Bounded by [`UNDER_CAP`]. It may read and write
    /// files and mint, so an async caller runs it on a blocking thread.
    pub fn leaf_under(&self, name: &str) -> Result<Arc<Leaf>, Error> {
        if !plain_host(name) {
            return Err(ca_err(format!("{name:?} is not a host name")));
        }
        let slot = self.under.lock().map_err(poisoned)?.slot(name);
        let mut held = slot.lock().map_err(poisoned)?;
        if let Some(l) = held.as_ref().filter(|l| l.fresh()) {
            return Ok(l.clone());
        }
        let dir = self.dir.join(CERTS).join(UNDER);
        let base = dir.join(file_stem(name));
        let leaf = match self.load_leaf(&base) {
            Some(l) => l,
            None => {
                std::fs::create_dir_all(&dir)?;
                let l = Arc::new(self.mint(name, &base, false)?);
                self.one_more_on_disk(&dir)?;
                l
            }
        };
        *held = Some(leaf.clone());
        Ok(leaf)
    }

    /// Counts a leaf just written, pruning the folder when that takes it past the cap. Counted from the folder the
    /// first time; a peer's writes are seen at the next prune, which counts again.
    fn one_more_on_disk(&self, dir: &Path) -> Result<(), Error> {
        let known = self.under.lock().map_err(poisoned)?.on_disk;
        let count = match known {
            Some(n) => n + 1,
            None => stems(dir).len(),
        };
        let count = if count > UNDER_CAP {
            prune(dir, UNDER_CAP)
        } else {
            count
        };
        self.under.lock().map_err(poisoned)?.on_disk = Some(count);
        Ok(())
    }
}

/// The leaves in `dir` by stem (a `.crt` and a `.key`, or what is left of a pair), each with its newest file's time;
/// stray temporary files older than [`STRAY_AFTER`] are removed on the way.
fn stems(dir: &Path) -> HashMap<String, (SystemTime, Vec<PathBuf>)> {
    let mut out: HashMap<String, (SystemTime, Vec<PathBuf>)> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let now = SystemTime::now();
    for e in entries.flatten() {
        let path = e.path();
        let Some(file) = path.file_name().and_then(|f| f.to_str()).map(str::to_owned) else {
            continue;
        };
        let at = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if file.contains(".tmp.") {
            if now.duration_since(at).is_ok_and(|age| age > STRAY_AFTER) {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        let Some(stem) = file
            .strip_suffix(".crt")
            .or_else(|| file.strip_suffix(".key"))
        else {
            continue;
        };
        let slot = out
            .entry(stem.to_owned())
            .or_insert((SystemTime::UNIX_EPOCH, Vec::new()));
        slot.0 = slot.0.max(at);
        slot.1.push(path);
    }
    out
}

/// Keeps at most `cap` leaves in `dir`, the oldest going with every file of theirs (a pair half written by a writer
/// that died included); the count left. Another program sharing the folder may prune at the same time; a file
/// already gone is fine.
fn prune(dir: &Path, cap: usize) -> usize {
    let mut all: Vec<(SystemTime, Vec<PathBuf>)> = stems(dir).into_values().collect();
    if all.len() <= cap {
        return all.len();
    }
    all.sort_by_key(|(at, _)| *at);
    let extra = all.len() - cap;
    for (_, files) in all.drain(..extra) {
        for f in files {
            let _ = std::fs::remove_file(f);
        }
    }
    all.len()
}

#[cfg(test)]
#[path = "ca_under_tests.rs"]
mod tests;
