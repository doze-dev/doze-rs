//! What an extension bundle says about itself, read with nothing else of the crate in it so the fuzz target can take it
//! whole: its `bundle.json` (the build's contract with the module), a control file's `default_version`, the ledger
//! of what doze placed into a shared toolchain (Go's `.doze-pgext.json`, read and written in Go's shape so either can
//! take over a toolchain the other installed into), and how two versions compare.

// fuzz:parser fuzz_bundle

use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// A bundle's `bundle.json`: what was built. Everything is optional; a missing field is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub name: String,
    pub version: String,
    pub pg_major: u64,
    pub preload: Vec<String>,
    pub settings: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

impl Manifest {
    pub(crate) fn parse(text: &str) -> Result<Manifest, String> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("bundle.json is not JSON ({e})"))?;
        let o = v.as_object().ok_or("bundle.json is not an object")?;
        let s = |k: &str| {
            o.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Manifest {
            name: s("name"),
            version: s("version"),
            pg_major: o.get("pg_major").and_then(Value::as_u64).unwrap_or(0),
            preload: strings(o.get("preload")),
            settings: pairs(o.get("settings")),
            env: pairs(o.get("env")),
        })
    }
}

/// `default_version` from a control file: the SQL version CREATE EXTENSION makes (pg_cron 1.6.4 makes 1.6).
pub(crate) fn default_version(control: &str) -> Option<String> {
    control.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        if k.trim() != "default_version" {
            return None;
        }
        let v = v.trim();
        let v = match v.strip_prefix('\'') {
            Some(q) => q.split('\'').next().unwrap_or_default(),
            None => v.split('#').next().unwrap_or_default().trim(),
        };
        let v = v.trim_matches('"');
        (!v.is_empty()).then(|| v.to_owned())
    })
}

/// Who placed one file into a toolchain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Owner {
    pub ext: String,
    pub version: String,
    /// The file's SHA-256 in hex, or `link:<target>` for a link.
    pub sha256: String,
}

/// The ledger: each file doze placed (`extension/x.control`, `lib/x.so`) and the bundle folders fully installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ledger {
    pub files: BTreeMap<String, Owner>,
    pub bundles: BTreeMap<String, bool>,
}

impl Ledger {
    pub(crate) fn parse(text: &str) -> Result<Ledger, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("it is not JSON ({e})"))?;
        let empty = Map::new();
        let files = v.get("files").and_then(Value::as_object).unwrap_or(&empty);
        let s = |o: &Value, k: &str| {
            o.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Ledger {
            files: files
                .iter()
                .map(|(k, o)| {
                    let owner = Owner {
                        ext: s(o, "ext"),
                        version: s(o, "version"),
                        sha256: s(o, "sha256"),
                    };
                    (k.clone(), owner)
                })
                .collect(),
            bundles: v
                .get("bundles")
                .and_then(Value::as_object)
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| (k.clone(), v.as_bool() == Some(true)))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    pub(crate) fn encode(&self) -> String {
        let files: Map<String, Value> = self
            .files
            .iter()
            .map(|(k, o)| {
                let v = serde_json::json!({"ext": o.ext, "version": o.version, "sha256": o.sha256});
                (k.clone(), v)
            })
            .collect();
        let mut v = serde_json::json!({ "files": files });
        if !self.bundles.is_empty()
            && let Some(o) = v.as_object_mut()
        {
            let b: Map<String, Value> = self
                .bundles
                .iter()
                .map(|(k, v)| (k.clone(), Value::Bool(*v)))
                .collect();
            o.insert("bundles".into(), Value::Object(b));
        }
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }
}

/// Dotted versions compared part by part: numbers as numbers (0.10 after 0.9), other parts as text, a missing part as 0.
pub(crate) fn compare(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        let (p, q) = match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (p, q) => (p.unwrap_or("0"), q.unwrap_or("0")),
        };
        let o = match (p.parse::<u64>(), q.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            _ => p.cmp(q),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_and_a_control_file() {
        let m = Manifest::parse(
            r#"{"name":"pg_cron","version":"1.6.8","pg_major":17,"preload":["pg_cron"],
                "settings":{"cron.database_name":"{db}"},"env":{},"requires":[]}"#,
        )
        .unwrap();
        assert_eq!(
            (m.name.as_str(), m.pg_major, m.preload.len()),
            ("pg_cron", 17, 1)
        );
        assert_eq!(
            m.settings.get("cron.database_name").map(String::as_str),
            Some("{db}")
        );
        assert!(Manifest::parse("[]").is_err() && Manifest::parse("{").is_err());
        let control = "# x\ncomment = 'a = b'\ndefault_version = '0.8.7'\nmodule_pathname = '$libdir/vector'\n";
        assert_eq!(default_version(control).as_deref(), Some("0.8.7"));
        assert_eq!(
            default_version("default_version = 1.6 # x").as_deref(),
            Some("1.6")
        );
        assert_eq!(default_version("comment = 'none'"), None);
    }

    #[test]
    fn the_ledger_reads_back_and_reads_gos() {
        let go = r#"{"files":{"extension/vector.control":{"ext":"vector","version":"0.8.7","sha256":"ab"}},
                    "bundles":{"/c/pgext-vector-pg17/0.8.7":true}}"#;
        let l = Ledger::parse(go).unwrap();
        assert_eq!(
            l.files
                .get("extension/vector.control")
                .map(|o| o.version.as_str()),
            Some("0.8.7")
        );
        assert_eq!(Ledger::parse(&l.encode()).unwrap(), l);
        assert_eq!(Ledger::parse("{}").unwrap(), Ledger::default());
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(compare("0.10.0", "0.9.9"), Ordering::Greater);
        assert_eq!(compare("0.8", "0.8.0"), Ordering::Equal);
        assert_eq!(compare("1.6", "1.6.1"), Ordering::Less);
        assert_eq!(compare("2.0beta", "2.0alpha"), Ordering::Greater);
    }
}
