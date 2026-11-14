//! `doze.lock`, read only: the module pins (`modules:`) the Go doze writes beside `doze.hcl`. Each pins a registry
//! source to one release, with the hashes of every platform's archive and the protocol, engine majors and oldest doze
//! copied from the signed index when it was pinned. Writing the lock is the core's (it pins on `up`).

use crate::registry::yaml::{self, YamlError, entries, get, text, texts};
use std::collections::BTreeMap;
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

/// The lock's module pins, by source (`doze/valkey`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lock {
    /// The newest doze that wrote it.
    pub doze: String,
    pub modules: BTreeMap<String, Pin>,
}

// fuzz:parser fuzz_lock
/// Reads a lock. Entries without a version (written before module pins had one) are left out, as Go drops them.
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
    let mut modules = BTreeMap::new();
    for (source, pin) in yaml::sub(&h, "modules")?.into_iter().flat_map(entries) {
        let Yaml::Hash(p) = pin else { continue };
        let version = text(p, "version");
        if version.is_empty() {
            continue;
        }
        let hashes = yaml::sub(p, "hashes")?
            .into_iter()
            .flat_map(entries)
            .filter_map(|(t, v)| yaml::scalar(v).map(|v| (t, v)))
            .collect();
        modules.insert(
            source,
            Pin {
                version,
                protocol: get(p, "protocol").as_i64().unwrap_or(0),
                engines: texts(p, "engines"),
                hashes,
                doze: text(p, "doze"),
            },
        );
    }
    Ok(Lock {
        doze: text(&h, "doze"),
        modules,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A lock as the Go doze writes it: engines layer, modules layer, downloads; only module pins are read.
    #[test]
    fn reads_the_module_pins_go_writes() {
        let src = "doze: 0.2.1\nengines: {}\nmodules:\n    doze/valkey:\n        version: 0.2.1\n        protocol: 1\n        engines:\n            - \"8\"\n            - \"9\"\n        hashes:\n            aarch64-apple-darwin: sha256:4d4c\n        doze: 0.2.0\n    doze/old:\n        stable: {}\n";
        let lock = parse(src).unwrap();
        assert_eq!(lock.doze, "0.2.1");
        assert_eq!(lock.modules.len(), 1, "a versionless pin is dropped");
        let pin = &lock.modules["doze/valkey"];
        assert_eq!((pin.version.as_str(), pin.protocol), ("0.2.1", 1));
        assert_eq!(pin.hashes["aarch64-apple-darwin"], "sha256:4d4c");
    }

    /// A merge left in the lock is the common way it breaks: said as that, at the line.
    #[test]
    fn a_merge_conflict_is_named_where_it_starts() {
        let src = "modules:\n<<<<<<< HEAD\n    doze/valkey:\n";
        let e = parse(src).unwrap_err();
        assert_eq!(
            (e.message.as_str(), e.at),
            ("a merge conflict was left in it", Some(9))
        );
    }
}
