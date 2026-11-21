//! Any text as a doze.hcl, in both passes: a load returns a config or a refusal, never panics,
//! never hangs, and every diagnostic points inside the source.
#![no_main]

use doze_config::modules::{ModuleInfo, Modules};
use doze_config::{Host, Options, parse};
use libfuzzer_sys::fuzz_target;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct FuzzHost;

impl Host for FuzzHost {
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        Arc::new(|name: &str| (name == "PATH").then(|| "/usr/bin:/bin".to_owned()))
    }
    fn cwd(&self) -> PathBuf {
        PathBuf::from("/nonexistent-doze-fuzz")
    }
    fn home_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/nonexistent-doze-fuzz/home"))
    }
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        if host.is_empty() { Err("empty".into()) } else { Ok(()) }
    }
    fn route_domain(&self, host: &str) -> String {
        host.strip_prefix("*.").unwrap_or(host).to_owned()
    }
    fn check_certificate(&self, _cert: &Path, _key: &Path, _domain: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Two module types: one with a version and a port, one built in.
struct FuzzModules;

impl Modules for FuzzModules {
    fn lookup(&self, ty: &str, _version: &str) -> Option<ModuleInfo> {
        self.cached(ty)
    }
    fn cached(&self, ty: &str) -> Option<ModuleInfo> {
        match ty {
            "postgres" => Some(ModuleInfo { versionless: false, default_port: 5432, supervised: false }),
            "aws" => Some(ModuleInfo { versionless: true, default_port: 4566, supervised: true }),
            _ => None,
        }
    }
    fn names(&self) -> Vec<String> {
        vec!["postgres".into(), "aws".into()]
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let opts = Options { host: &FuzzHost, modules: &FuzzModules, vars: Default::default(), version: "dev".into() };
    for shape in [false, true] {
        if let Err(e) = parse(text, "doze.hcl", &opts, shape) {
            for d in e.diags.iter() {
                let at = d.subject.unwrap_or_default();
                assert!(at.start.byte as usize <= text.len() && at.end.byte as usize <= text.len());
            }
        }
    }
});
