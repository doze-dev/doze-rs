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
    /// A project in the system's temporary folder is refused when the home is not in it too: that is a test's or a
    /// fixture's project run against the person's real home, and its domains would be routed machine-wide (a fixture's
    /// `dev.acme.com` once landed in a real record this way). Folders whose doze.hcl is gone leave every entry here.
    pub fn record(&self, dir: &str, domains: &[String]) -> Result<(), Error> {
        if throwaway(&self.home, dir) {
            return Err(Error::Throwaway(dir.to_owned()));
        }
        let home = self.home.clone();
        let want: BTreeSet<String> = route_domains(domains)
            .into_iter()
            .filter(|d| valid_domain(d).is_ok())
            .collect();
        let routed = self.routed;
        self.update(|rs| {
            rs.retain(|d, dirs| {
                dirs.retain(|x| x != dir && claims(&home, x));
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
        dirs.iter().any(|d| claims(&self.home, d))
    }
}

/// Whether a recorded folder may still mean its routes: it has a doze.hcl, and it is not a throwaway folder in a real
/// home (one recorded before [`Routes::record`] refused them is treated as gone, so its route goes as stale).
fn claims(home: &Path, dir: &str) -> bool {
    Path::new(dir).join("doze.hcl").is_file() && !throwaway(home, dir)
}

/// The system's temporary folder, as named and with links resolved (macOS: `/var/folders/…` is
/// `/private/var/folders/…`, and project folders are recorded resolved).
fn temp_roots() -> Vec<PathBuf> {
    let t = std::env::temp_dir();
    let mut out = vec![t.clone()];
    if let Ok(c) = t.canonicalize()
        && c != t
    {
        out.push(c);
    }
    out
}

/// Whether `dir` is in the system's temporary folder while `home` is not: a test's or a fixture's project reaching
/// the person's real home.
pub(crate) fn throwaway(home: &Path, dir: &str) -> bool {
    let roots = temp_roots();
    let under = |p: &Path| roots.iter().any(|t| p.starts_with(t));
    let home_resolved = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    under(Path::new(dir)) && !under(home) && !under(&home_resolved)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn project(root: &Path, name: &str) -> String {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("doze.hcl"), "stack = \"x\"\n").unwrap();
        dir.canonicalize().unwrap().display().to_string()
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dz-routes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_throwaway_project_never_reaches_a_real_home() {
        let tmp = scratch("real");
        let acme = project(&tmp, "work/acme");
        // A home outside the temporary folder stands for the person's real ~/.doze. Nothing is written to it: the
        // refusal comes before the file is touched.
        let real = Path::new("/nonexistent-doze-home-for-a-test");
        let routes = Routes::with_check(real, |_| false);
        let err = routes.record(&acme, &["dev.acme.com".into()]).unwrap_err();
        assert!(matches!(err, Error::Throwaway(_)), "{err}");
        assert!(err.to_string().contains("temporary folder"), "{err}");
        assert!(!real.exists());
        // A test's own home in the temporary folder records its projects as before.
        let home = tmp.join("home");
        let routes = Routes::with_check(&home, |_| false);
        routes.record(&acme, &["dev.acme.com".into()]).unwrap();
        assert_eq!(routes.claimants("dev.acme.com"), vec![acme.clone()]);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn recording_drops_folders_that_are_gone_and_a_throwaway_entry_reads_as_stale() {
        let tmp = scratch("gone");
        let home = tmp.join("home");
        let (a, b) = (project(&tmp, "a"), project(&tmp, "b"));
        let routes = Routes::with_check(&home, |d| d == "old.test");
        routes
            .record(&a, &["a.test".into(), "old.test".into()])
            .unwrap();
        std::fs::remove_dir_all(&a).unwrap();
        routes.record(&b, &["b.test".into()]).unwrap();
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

        // An entry written before the refusal (a fixture's folder in a real home) counts as gone.
        assert!(throwaway(
            Path::new("/nonexistent-doze-home-for-a-test"),
            &b
        ));
        assert!(!throwaway(&home, &b));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
