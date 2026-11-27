//! Which project asked for each domain route. A route is machine-wide and privileged to install, so it must be known who
//! still wants it: when a domain leaves a project's doze.hcl, or the project folder is gone, its route is stale and the
//! next privileged run (`doze trust`, or setup's uninstall) removes it, and doctor says so until then. Nothing in
//! `/etc/resolver` that this file does not list is ever touched.
//!
//! The file is Go's `routes.json` (domain to the project folders that declare it), shared with Go programs on the
//! machine: same name, same shape, same lock.

use crate::Error;
use crate::ca::write_atomic;
use crate::domains::{route_domains, valid_domain, wildcard};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const FILE: &str = "routes.json";

/// Domain to the project folders that declare it.
type Record = BTreeMap<String, Vec<String>>;

/// The routes record under a doze home.
#[derive(Debug, Clone)]
pub struct Routes {
    home: PathBuf,
    /// Whether the machine routes a domain now: the platform's check, or a test's stand-in (tests run with no
    /// privilege and no routes).
    routed: fn(&str) -> bool,
}

impl Routes {
    /// The record under `home`, asking the machine whether a domain is routed.
    pub fn open(home: &Path) -> Routes {
        Routes::with_check(home, |d| crate::trust_store::domain_routed(d).0)
    }

    /// The record under `home`, with `routed` standing in for the machine.
    pub fn with_check(home: &Path, routed: fn(&str) -> bool) -> Routes {
        Routes {
            home: home.to_path_buf(),
            routed,
        }
    }

    /// Where the record is.
    pub fn path(&self) -> PathBuf {
        self.home.join(FILE)
    }

    /// The record. A key that is not a valid domain is dropped: the file is the person's to edit, and its keys end up
    /// in a privileged script.
    fn read(&self) -> Record {
        let text = std::fs::read_to_string(self.path()).unwrap_or_default();
        let mut rs: Record = serde_json::from_slice(text.as_bytes()).unwrap_or_default();
        rs.retain(|d, _| valid_domain(d).is_ok() && !wildcard(d));
        rs
    }

    /// Runs `f` on the record under a lock, as the registry does, so two daemons starting together both land in it.
    fn update(&self, f: impl FnOnce(&mut Record)) -> Result<(), Error> {
        std::fs::create_dir_all(&self.home)?;
        let lock = PathBuf::from(format!("{}.lock", self.path().display()));
        let _guard = doze_os::fs::lock(&lock)?;
        let mut rs = self.read();
        f(&mut rs);
        let mut text = serde_json::to_string_pretty(&rs).map_err(std::io::Error::other)?;
        text.push('\n');
        write_atomic(&self.path(), text.as_bytes(), false)
    }

    /// What a project declares now, replacing what it said before: a domain it dropped loses this project as a reason
    /// to be routed. One the machine still routes is kept with no claimant, so [`Routes::stale`] reports it until the
    /// route is removed; one never routed is forgotten.
    ///
    /// A deleted project (its folder missing while the folder it was in is there) leaves every entry here; one that
    /// only cannot be seen now (an unmounted volume, a checkout in progress) is kept.
    pub fn record(&self, dir: &str, domains: &[String]) -> Result<(), Error> {
        let want: BTreeSet<String> = route_domains(domains)
            .into_iter()
            .filter(|d| valid_domain(d).is_ok())
            .collect();
        let routed = self.routed;
        self.update(|rs| {
            rs.retain(|d, dirs| {
                dirs.retain(|x| x != dir && !gone(x));
                !dirs.is_empty() || want.contains(d) || routed(d)
            });
            for d in want {
                rs.entry(d).or_default().push(dir.to_string());
            }
        })
    }

    /// Drops a project from every route it declared: the folder is gone, or `doze trust --remove` ran for it.
    pub fn forget_project(&self, dir: &str) -> Result<(), Error> {
        self.update(|rs| {
            for dirs in rs.values_mut() {
                dirs.retain(|x| x != dir);
            }
        })
    }

    /// Every domain this machine was asked to route, with or without a claimant left, sorted.
    pub fn recorded(&self) -> Vec<String> {
        self.read().into_keys().collect()
    }

    /// The routed domains no project declares any more: every claimant dropped it, or every claimant's folder is gone.
    /// Folders are checked here, so a deleted project needs no step of its own.
    pub fn stale(&self) -> Vec<String> {
        self.read()
            .into_iter()
            .filter(|(d, dirs)| !self.declared(dirs) && (self.routed)(d))
            .map(|(d, _)| d)
            .collect()
    }

    /// Forgets a stale domain once its route is gone.
    pub fn removed(&self, domain: &str) -> Result<(), Error> {
        self.update(|rs| {
            if rs.get(domain).is_some_and(|dirs| !self.declared(dirs)) {
                rs.remove(domain);
            }
        })
    }

    /// Whether some project with a doze.hcl still declares a domain: the authority stays trusted while one does.
    pub fn any_declared(&self) -> bool {
        self.read().values().any(|dirs| self.declared(dirs))
    }

    /// The projects with a doze.hcl that still declare `route`.
    pub fn claimants(&self, route: &str) -> Vec<String> {
        let mut rs = self.read();
        let dirs = rs.remove(route).unwrap_or_default();
        dirs.into_iter()
            .filter(|d| self.declared(std::slice::from_ref(d)))
            .collect()
    }

    /// Every route the machine should keep after a run: `mine`, plus what other projects still declare, minus `prune`.
    pub fn keep(&self, mine: &[String], prune: &[String]) -> Vec<String> {
        let mut keep: BTreeSet<String> = mine.iter().cloned().collect();
        keep.extend(
            self.read()
                .into_iter()
                .filter(|(_, dirs)| self.declared(dirs))
                .map(|(d, _)| d),
        );
        for d in prune {
            keep.remove(d);
        }
        keep.into_iter().collect()
    }

    /// Removes the record and its lock (setup's uninstall, after the privileged part ran).
    pub fn remove_all(&self) {
        let _ = std::fs::remove_file(self.path());
        let _ = std::fs::remove_file(format!("{}.lock", self.path().display()));
    }
}

impl Routes {
    /// Whether one of the folders still claims its routes ([`claims`]).
    fn declared(&self, dirs: &[String]) -> bool {
        dirs.iter().any(|d| claims(d))
    }

    /// The routes recorded for a project in the system's temporary folder while this home is not in it: a test's or a
    /// fixture's run against the person's home (a fixture's `dev.acme.com` once landed in a real record this way).
    /// Said by doctor as a leftover with the command that removes it, never dropped silently: a person may keep a
    /// project there on purpose. `(domain, folder)`, sorted.
    pub fn temporary(&self) -> Vec<(String, String)> {
        if under_temp(&self.home) {
            return Vec::new();
        }
        self.read()
            .into_iter()
            .flat_map(|(d, dirs)| dirs.into_iter().map(move |x| (d.clone(), x)))
            .filter(|(_, x)| under_temp(Path::new(x)) && claims(x))
            .collect()
    }
}

/// Whether a recorded folder may still mean its routes: it holds a config doze would load (any `*.hcl`: `-c` takes
/// another name than doze.hcl). Only a folder that is certainly gone or certainly holds none stops claiming: one that
/// cannot be read now (permissions, a volume being mounted) still claims, so no route is removed on a passing error.
fn claims(dir: &str) -> bool {
    match std::fs::metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
        Ok(m) if !m.is_dir() => false,
        Ok(_) => std::fs::read_dir(dir).map_or(true, |entries| {
            entries.flatten().any(|e| {
                e.path().extension().is_some_and(|x| x == "hcl")
                    && e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink())
            })
        }),
    }
}

/// Whether a recorded folder is gone for good, so [`Routes::record`] may drop it: it does not exist while the folder
/// it was in does (a deleted project). A missing parent too reads as an unmounted volume or a checkout in progress,
/// and the entry is kept.
fn gone(dir: &str) -> bool {
    let path = Path::new(dir);
    matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        && path.parent().is_some_and(Path::is_dir)
}

/// Whether `p` is in the system's temporary folder (as named, and with links resolved: macOS's `/var/folders/…` is
/// `/private/var/folders/…`, and project folders are recorded resolved).
fn under_temp(p: &Path) -> bool {
    let t = std::env::temp_dir();
    let roots = [Some(t.clone()), t.canonicalize().ok()];
    let resolved = p.canonicalize().ok();
    roots
        .iter()
        .flatten()
        .any(|root| p.starts_with(root) || resolved.as_ref().is_some_and(|r| r.starts_with(root)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn project(root: &Path, name: &str, file: &str) -> String {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), "stack = \"x\"\n").unwrap();
        dir.canonicalize().unwrap().display().to_string()
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dz-routes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_project_under_any_config_name_keeps_its_route_and_a_deleted_one_goes() {
        let tmp = scratch("claims");
        let home = tmp.join("home");
        let routes = Routes::with_check(&home, |d| d == "old.test");
        // `-c stack.hcl`: no doze.hcl, still a project doze loads.
        let other = project(&tmp, "other", "stack.hcl");
        let a = project(&tmp, "a", "doze.hcl");
        routes.record(&other, &["other.test".into()]).unwrap();
        routes
            .record(&a, &["a.test".into(), "old.test".into()])
            .unwrap();
        std::fs::remove_dir_all(&a).unwrap();
        routes
            .record(&project(&tmp, "b", "doze.hcl"), &["b.test".into()])
            .unwrap();
        assert_eq!(
            routes.claimants("other.test"),
            vec![other.clone()],
            "another project's record keeps it"
        );
        let text = std::fs::read_to_string(routes.path()).unwrap();
        assert!(
            !text.contains(&a),
            "a deleted project leaves the record: {text}"
        );
        assert!(
            !text.contains("a.test"),
            "its unrouted domain goes with it: {text}"
        );
        assert_eq!(
            routes.stale(),
            vec!["old.test".to_string()],
            "a routed one stays, stale"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_folder_that_cannot_be_seen_now_is_never_pruned() {
        let tmp = scratch("unmounted");
        let home = tmp.join("home");
        // As an unmounted volume reads: the folder and its parent are both missing.
        let away = tmp.join("Volumes/ext/shop").display().to_string();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(FILE), format!("{{\"shop.test\": [{away:?}]}}\n")).unwrap();
        let routes = Routes::with_check(&home, |_| true);
        routes
            .record(&project(&tmp, "b", "doze.hcl"), &["b.test".into()])
            .unwrap();
        let text = std::fs::read_to_string(routes.path()).unwrap();
        assert!(text.contains(&away), "kept while it cannot be seen: {text}");
        assert!(!gone(&away));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_temporary_project_in_a_real_home_is_a_leftover_not_dropped() {
        let tmp = scratch("temporary");
        let acme = project(&tmp, "work/acme", "doze.hcl");
        // An entry as the Go fixture left it: a temporary project in a home outside the temporary folder.
        let real = std::env::current_dir()
            .unwrap()
            .join(".routes-temporary-test-home");
        let routes = Routes::with_check(&real, |_| true);
        assert!(!under_temp(&real));
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(
            real.join(FILE),
            format!("{{\"dev.acme.com\": [{acme:?}]}}\n"),
        )
        .unwrap();
        assert_eq!(
            routes.temporary(),
            vec![("dev.acme.com".to_string(), acme.clone())]
        );
        assert!(
            routes.stale().is_empty(),
            "it still claims its route: said, not removed"
        );
        assert_eq!(routes.claimants("dev.acme.com"), vec![acme.clone()]);
        let _ = std::fs::remove_dir_all(&real);
        // A test's own home in the temporary folder has no such leftover.
        let home = tmp.join("home");
        let routes = Routes::with_check(&home, |_| false);
        routes.record(&acme, &["dev.acme.com".into()]).unwrap();
        assert!(routes.temporary().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
