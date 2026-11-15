//! A region's functions as data (`doze-aws data`): each function's code packages (the archive and the tree unpacked from it, under
//! `code/<id>`) across `$LATEST` and its published versions, and its rows; each layer's the same; the code nothing points at any
//! more; and, in the home region, the Runtime API clients written for Python, Node and Ruby functions.
//!
//! A function's code is what it runs, so it is not "emptied": deleting the function (or one published version) is the AWS call for
//! that. What can go without changing anything a function does is the code nothing uses (`unused`), and everything (`reset`).
//! A function writes its scratch files where its process runs, its unpacked tree, so they are counted with its code.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aws_wire::storage::{Emptied, Prune, StorageError};
use doze_store::record;

use crate::core::{Core, VERSION};

/// The resource that stands for the code no function, version or layer points at.
pub(crate) const UNUSED: &str = "(unused code)";
/// The resource that stands for the Runtime API clients doze-aws writes (home region only).
pub(crate) const CLIENTS: &str = "(runtime clients)";

/// How a function, a layer or a prune that cannot apply is refused.
const WAYS: &str = "a function's code is what it runs, so it is not emptied: DeleteFunction removes a function and its code \
(with a Qualifier, one published version), DeleteLayerVersion a layer version; --unused removes the code nothing uses";

/// The bytes the files under `path` hold (symbolic links are not followed).
pub(crate) fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map_or(0, |read| {
        read.filter_map(Result::ok)
            .map(|e| size_of(&e.path()))
            .sum()
    })
}

fn remove(path: &Path) -> u64 {
    let bytes = size_of(path);
    let gone = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    if gone.is_ok() { bytes } else { 0 }
}

pub(crate) fn encoded<T: serde::Serialize>(what: &str, key: &str, value: &T) -> u64 {
    (key.len() + record::encode(what, VERSION, value).map_or(0, |b| b.len())) as u64
}

impl Core {
    pub(crate) fn code_dir(&self) -> PathBuf {
        self.dir.join("code")
    }

    pub(crate) fn clients_dir(&self) -> Option<PathBuf> {
        (self.region == crate::HOME_REGION).then(|| crate::rt::shims::dir(&self.dir))
    }

    /// The code directories nothing points at, with their bytes; those changed within `grace` are left (an operation may be about
    /// to take one: code is unpacked before the actor is asked to keep it).
    pub(crate) fn unused_code(&self, grace: Duration) -> Vec<(PathBuf, u64)> {
        let keep = self.referenced_code();
        let Ok(read) = std::fs::read_dir(self.code_dir()) else {
            return Vec::new();
        };
        let now = SystemTime::now();
        read.filter_map(Result::ok)
            .filter(|e| !keep.contains(&*e.file_name().to_string_lossy()))
            .filter(|e| {
                e.metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|at| now.duration_since(at).unwrap_or_default() >= grace)
            })
            .map(|e| (e.path(), size_of(&e.path())))
            .collect()
    }

    /// Removes what `prune` names: only the code nothing uses (and the runtime clients) can go.
    pub(crate) fn empty(
        &self,
        name: &str,
        prune: &Prune,
        grace: Duration,
    ) -> Result<Emptied, StorageError> {
        if prune.prefix.is_some() || prune.old_versions || prune.before_ms.is_some() {
            return Err(StorageError::Unsupported(WAYS.to_owned()));
        }
        let leftover = name == UNUSED || name == CLIENTS;
        if !prune.unused && !leftover {
            let known = self.functions.get(name).is_some()
                || name
                    .strip_prefix("layer:")
                    .is_some_and(|l| self.layers.get(l).is_some());
            return Err(if known {
                StorageError::Unsupported(WAYS.to_owned())
            } else {
                StorageError::NotFound
            });
        }
        let mut gone = self.unused_code(grace);
        if let Some(dir) = self.clients_dir().filter(|d| d.is_dir()) {
            let bytes = size_of(&dir);
            gone.push((dir, bytes));
        }
        let mut done = Emptied::default();
        for (_, bytes) in &gone {
            done.items += 1;
            done.bytes += bytes;
        }
        if !prune.dry_run {
            done.freed = gone.iter().map(|(path, _)| remove(path)).sum();
        }
        Ok(done)
    }

    /// Removes every function, layer, mapping, code signing configuration and checkpoint of the region, and all code. Answers the
    /// functions that were there, whose processes are to be ended.
    pub(crate) async fn reset(&mut self, dry_run: bool) -> Result<(Emptied, Vec<String>), String> {
        let mut done = Emptied::default();
        let mut count = |bytes: u64| {
            done.items += 1;
            done.bytes += bytes;
        };
        self.functions
            .iter()
            .for_each(|(k, v)| count(encoded("function", k, v)));
        self.layers
            .iter()
            .for_each(|(k, v)| count(encoded("layer", k, v)));
        self.mappings
            .iter()
            .for_each(|(k, v)| count(encoded("event source mapping", k, v)));
        self.signing
            .iter()
            .for_each(|(k, v)| count(encoded("code signing configuration", k, v)));
        self.checkpoints
            .iter()
            .for_each(|(k, v)| count(encoded("checkpoint", k, v)));
        let files: Vec<PathBuf> = [Some(self.code_dir()), self.clients_dir()]
            .into_iter()
            .flatten()
            .collect();
        done.bytes += files.iter().map(|p| size_of(p)).sum::<u64>();
        let names: Vec<String> = self.functions.iter().map(|(k, _)| k.to_owned()).collect();
        if dry_run {
            return Ok((done, names));
        }
        self.forget_all();
        let saved = self.flush().map_err(|e| format!("could not save: {e}"))?;
        if let Some(ack) = saved {
            ack.await.map_err(|e| format!("could not save: {e}"))?;
        }
        // The rows are on disk first: a crash now leaves code nothing points at, which `unused` and the next delete sweep.
        done.freed = files.iter().map(|p| remove(p)).sum::<u64>() + self.compact()?;
        Ok((done, names))
    }

    fn forget_all(&mut self) {
        fn clear<T: serde::Serialize + serde::de::DeserializeOwned>(
            r: &mut doze_store::records::Records<T>,
        ) {
            let keys: Vec<String> = r.iter().map(|(k, _)| k.to_owned()).collect();
            for k in keys {
                r.remove(&k);
            }
        }
        clear(&mut self.functions);
        clear(&mut self.layers);
        clear(&mut self.mappings);
        clear(&mut self.signing);
        clear(&mut self.checkpoints);
    }

    /// Compacts the file; answers the bytes freed.
    pub(crate) fn compact(&mut self) -> Result<u64, String> {
        match self.db.as_mut() {
            Some(db) => db.compact().map(|c| c.freed()).map_err(|e| e.to_string()),
            None => Ok(0),
        }
    }
}
