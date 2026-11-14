//! What an instance's bundles add to its server (Go's `preload.go`): libraries to preload, settings they need, where
//! PostgreSQL 18 finds them (`extension_control_path`, `dynamic_library_path`), and environment for the server. The
//! person's own value for a list setting is merged after doze's, not replaced, and their own value for any other setting
//! wins.

use crate::bundle;
use crate::catalog;
use crate::spec::{Ext, PostgresSpec};
use doze_module::proto::msg::instance::Toolchain;
use std::collections::BTreeMap;

/// List settings doze merges with the person's own.
pub(crate) const MERGED: [&str; 3] = [
    "shared_preload_libraries",
    "extension_control_path",
    "dynamic_library_path",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExtConf {
    /// `shared_preload_libraries`, in load order.
    pub preload: Vec<String>,
    pub settings: BTreeMap<String, String>,
    /// PostgreSQL 18's search paths: each bundle's `share` and `lib`.
    pub control_path: Vec<String>,
    pub lib_path: Vec<String>,
    pub env: BTreeMap<String, String>,
}

fn push(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|l| l == item) {
        list.push(item.to_owned());
    }
}

/// The bundle names the instance uses, in creation order, each once.
fn bundles(exts: Vec<Ext>, major: u32) -> Vec<String> {
    let mut out = Vec::new();
    for e in exts {
        if e.source.is_some() {
            push(&mut out, &e.name);
            continue;
        }
        if catalog::is_contrib(&e.name, major) {
            continue;
        }
        let Some(r) = catalog::lookup(&e.name) else {
            continue;
        };
        for req in catalog::requires_of(&r) {
            if let Some(rr) = catalog::lookup(req).filter(|_| !catalog::is_contrib(req, major)) {
                push(&mut out, rr.entry.name);
            }
        }
        push(&mut out, r.entry.name);
    }
    out
}

/// The instance's extension configuration from the bundles its toolchain holds.
pub(crate) fn ext_conf(s: &PostgresSpec, tc: &Toolchain, major: u32, db: &str) -> ExtConf {
    let mut c = ExtConf::default();
    for name in bundles(s.exts(), major) {
        let Some(root) = bundle::root(tc, &name) else {
            continue;
        };
        let m = bundle::manifest(&root, &name);
        let (mut preload, mut settings, mut env) = (Vec::new(), BTreeMap::new(), BTreeMap::new());
        if let Some(e) = catalog::bundle(&name) {
            preload.extend(e.preload.iter().map(|p| (*p).to_owned()));
            settings.extend(
                e.settings
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
            );
            env.extend(
                e.env
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
            );
        }
        // bundle.json is what was built: it adds to the catalog, and wins over it.
        preload.extend(m.preload);
        settings.extend(m.settings);
        env.extend(m.env);
        for p in preload {
            push(&mut c.preload, &p);
        }
        for (k, v) in settings {
            c.settings.insert(k, v.replace("{db}", db));
        }
        let r = root.to_string_lossy();
        for (k, v) in env {
            c.env.insert(k, v.replace("{root}", &r));
        }
        if major >= 18 {
            c.control_path
                .push(root.join("share").to_string_lossy().into_owned());
            c.lib_path
                .push(root.join("lib").to_string_lossy().into_owned());
        }
    }
    c.preload = ordered(c.preload);
    c
}

/// timescaledb first (it refuses to load after others), the rest in declaration order.
fn ordered(mut libs: Vec<String>) -> Vec<String> {
    libs.sort_by_key(|l| l != "timescaledb");
    libs
}

/// A list setting's items: split, trimmed, unquoted, empty ones dropped.
pub(crate) fn items(v: &str, sep: char) -> Vec<String> {
    v.split(sep)
        .map(|p| p.trim().trim_matches(|c| c == '"' || c == '\''))
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// doze's entries, then the person's (or the default token), each once.
fn joined(ours: &[String], theirs: Option<&str>, sep: char, default: &str) -> String {
    let mut all = ours.to_vec();
    let rest = theirs.map(|t| items(t, sep)).unwrap_or_default();
    if rest.is_empty() && !default.is_empty() {
        push(&mut all, default);
    }
    for r in rest {
        push(&mut all, &r);
    }
    all.join(&sep.to_string())
}

impl ExtConf {
    /// The lines this adds to doze.conf, unquoted: the merged list settings (when either side has one), then each
    /// setting the person did not set themselves. `user` is the person's settings.
    pub(crate) fn lines(&self, user: &BTreeMap<String, String>) -> Vec<(String, String)> {
        let theirs = |key: &str| {
            user.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str())
        };
        let mut out = Vec::new();
        let lists: [(&str, &[String], char, &str); 3] = [
            ("shared_preload_libraries", &self.preload, ',', ""),
            ("extension_control_path", &self.control_path, ':', "$system"),
            ("dynamic_library_path", &self.lib_path, ':', "$libdir"),
        ];
        for (key, ours, sep, default) in lists {
            let user_value = theirs(key);
            if ours.is_empty() && user_value.is_none() {
                continue;
            }
            let value = if ours.is_empty() {
                user_value.unwrap_or_default().to_owned()
            } else {
                let mut libs = items(&joined(ours, user_value, sep, default), sep);
                if key == "shared_preload_libraries" {
                    libs = ordered(libs);
                }
                libs.join(&sep.to_string())
            };
            out.push((key.to_owned(), value));
        }
        for (k, v) in &self.settings {
            if theirs(k).is_none() {
                out.push((k.clone(), v.clone()));
            }
        }
        out
    }

    /// The libraries this wants preloaded that a running server's `SHOW shared_preload_libraries` lacks.
    pub(crate) fn missing(&self, have: &str) -> Vec<String> {
        let got = items(have, ',');
        self.preload
            .iter()
            .filter(|w| {
                !got.iter()
                    .any(|g| g == *w || g.strip_prefix("$libdir/") == Some(w.as_str()))
            })
            .cloned()
            .collect()
    }
}

/// The `shared_preload_libraries` line doze.conf in `data_dir` has, empty when none.
pub(crate) fn preload_line(data_dir: &str) -> String {
    let conf = std::path::Path::new(data_dir).join("doze.conf");
    std::fs::read_to_string(conf)
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "shared_preload_libraries").then(|| v.trim().trim_matches('\'').to_owned())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conf() -> ExtConf {
        ExtConf {
            preload: vec!["pg_cron".into(), "timescaledb".into()],
            settings: BTreeMap::from([
                ("cron.database_name".into(), "app".into()),
                ("x.y".into(), "1".into()),
            ]),
            control_path: vec!["/b/share".into()],
            lib_path: vec!["/b/lib".into()],
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn list_settings_merge_and_the_persons_own_win() {
        let mut c = conf();
        c.preload = ordered(c.preload);
        let user = BTreeMap::from([
            (
                "Shared_Preload_Libraries".into(),
                "auto_explain, pg_cron".into(),
            ),
            ("x.y".into(), "2".into()),
        ]);
        let lines = c.lines(&user);
        let get = |k: &str| lines.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(
            get("shared_preload_libraries"),
            Some("timescaledb,pg_cron,auto_explain")
        );
        assert_eq!(get("extension_control_path"), Some("/b/share:$system"));
        assert_eq!(get("dynamic_library_path"), Some("/b/lib:$libdir"));
        assert_eq!(get("cron.database_name"), Some("app"));
        assert_eq!(
            get("x.y"),
            None,
            "the person's own is written with their settings"
        );
        assert!(ExtConf::default().lines(&BTreeMap::new()).is_empty());
        let only_theirs = BTreeMap::from([("dynamic_library_path".into(), "/mine:$libdir".into())]);
        assert_eq!(
            ExtConf::default().lines(&only_theirs),
            [(
                "dynamic_library_path".to_string(),
                "/mine:$libdir".to_string()
            )]
        );
    }

    #[test]
    fn a_running_server_missing_a_library() {
        let c = conf();
        assert_eq!(c.missing("'$libdir/pg_cron'"), ["timescaledb"]);
        assert!(c.missing("timescaledb, pg_cron").is_empty());
    }
}
