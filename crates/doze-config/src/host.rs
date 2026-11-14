//! What a load reads from the machine it runs on: the environment, the working folder, the home
//! folder, and the domain and certificate checks (which belong to doze-names). Tests give the
//! oracle's machine; the CLI gives [`OsHost`].

use std::path::{Path, PathBuf};

pub trait Host {
    /// The environment (`DOZE_HOME`, `DOZE_ZONE`, `DOZE_VAR_<name>`, and `env(…)` in expressions).
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup;
    /// One environment variable.
    fn env(&self, name: &str) -> Option<String> {
        (self.env_lookup())(name)
    }
    /// The folder relative paths are read against (Go's process working directory).
    fn cwd(&self) -> PathBuf;
    /// The user's home folder (Go's `os.UserHomeDir`).
    fn home_dir(&self) -> Option<PathBuf>;
    /// doze-names' `ValidDomain`: the reason `host` is not a domain name an app may declare.
    fn valid_domain(&self, host: &str) -> Result<(), String>;
    /// doze-names' `RouteDomain`: the name a wildcard (`*.x`) is routed under.
    fn route_domain(&self, host: &str) -> String;
    /// doze-names' `CheckCertificate`: the reason the pair does not serve `domain`.
    fn check_certificate(&self, cert: &Path, key: &Path, domain: &str) -> Result<(), String>;

    /// `path` made absolute against [`Host::cwd`] (Go's `filepath.Abs`), lexically cleaned.
    fn abs(&self, path: &Path) -> PathBuf {
        crate::paths::clean(&if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd().join(path)
        })
    }
}

/// `DOZE_ZONE` switches names off (`off`, `0`, `false`, `no`; doze-names' `Disabled`).
pub fn names_disabled(host: &dyn Host) -> bool {
    host.env("DOZE_ZONE").is_some_and(|z| {
        matches!(
            z.trim().to_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

/// The process's own environment, working folder and home, with doze-names' checks given by the
/// caller (doze-config does not link doze-names).
pub struct OsHost<'a> {
    pub domains: &'a dyn DomainChecks,
}

/// The doze-names half of [`Host`].
pub trait DomainChecks {
    fn valid_domain(&self, host: &str) -> Result<(), String>;
    fn route_domain(&self, host: &str) -> String;
    fn check_certificate(&self, cert: &Path, key: &Path, domain: &str) -> Result<(), String>;
}

impl std::fmt::Debug for OsHost<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OsHost")
    }
}

impl Host for OsHost<'_> {
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        std::sync::Arc::new(|name: &str| {
            if name.is_empty() || name.contains(['=', '\0']) {
                return None;
            }
            std::env::var(name).ok()
        })
    }
    fn cwd(&self) -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    }
    fn home_dir(&self) -> Option<PathBuf> {
        std::env::home_dir()
    }
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        self.domains.valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        self.domains.route_domain(host)
    }
    fn check_certificate(&self, cert: &Path, key: &Path, domain: &str) -> Result<(), String> {
        self.domains.check_certificate(cert, key, domain)
    }
}
