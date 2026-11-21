//! Reading doze.hcl: the stack, or one block alone, through doze-config with this process's machine and doze-names'
//! domain checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The stack at `path`, as the CLI loads it: this process's environment (`DOZE_VAR_*` included), doze-names' checks.
pub(super) fn load(
    path: &str,
    version: &str,
    modules: &dyn doze_config::modules::Modules,
    env: BTreeMap<String, String>,
) -> Result<doze_config::Config, String> {
    with_options(version, modules, env, |opts| doze_config::load(path, opts))
}

/// One block read alone as if it were in the stack at `path` (Go's `Parse` of the add op's block): its references to
/// other instances do not resolve.
pub(crate) fn parse_block(
    block: &str,
    path: &str,
    version: &str,
    modules: &dyn doze_config::modules::Modules,
    env: BTreeMap<String, String>,
) -> Result<doze_config::Config, String> {
    with_options(version, modules, env, |opts| {
        doze_config::parse(block, path, opts, false)
    })
}

pub(crate) fn with_options(
    version: &str,
    modules: &dyn doze_config::modules::Modules,
    env: BTreeMap<String, String>,
    f: impl FnOnce(&doze_config::Options<'_>) -> Result<doze_config::Config, doze_config::Error>,
) -> Result<doze_config::Config, String> {
    let domains = Domains;
    let host = Overlay {
        os: doze_config::host::OsHost { domains: &domains },
        env: Arc::new(env),
    };
    let opts = doze_config::Options {
        host: &host,
        modules,
        vars: BTreeMap::new(),
        version: version.into(),
    };
    f(&opts).map_err(|e| e.to_string())
}

/// This process's machine, with some variables set first.
struct Overlay<'a> {
    os: doze_config::host::OsHost<'a>,
    env: Arc<BTreeMap<String, String>>,
}

impl doze_config::host::Host for Overlay<'_> {
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        let (os, env) = (self.os.env_lookup(), self.env.clone());
        Arc::new(move |name: &str| env.get(name).cloned().or_else(|| os(name)))
    }
    fn cwd(&self) -> PathBuf {
        self.os.cwd()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.os.home_dir()
    }
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        self.os.valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        self.os.route_domain(host)
    }
    fn check_certificate(&self, cert: &Path, key: &Path, domain: &str) -> Result<(), String> {
        self.os.check_certificate(cert, key, domain)
    }
}

/// doze-names' domain checks for the config.
struct Domains;

impl doze_config::host::DomainChecks for Domains {
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        doze_names::domains::valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        doze_names::domains::route_domain(host)
    }
    fn check_certificate(&self, cert: &Path, key: &Path, _domain: &str) -> Result<(), String> {
        for f in [cert, key] {
            if !f.exists() {
                return Err(format!("{} does not exist", f.display()));
            }
        }
        Ok(())
    }
}
