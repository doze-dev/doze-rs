//! `doze __fetch` (hidden): one download for the daemon, in a process of its own, so the daemon never holds an HTTPS
//! client (`doze_fetch::child`). One request line on stdin, one reply line on stdout, and a download said on stderr
//! (the daemon's log) as it starts.

use crate::ctx::VERSION;
use crate::registry;
use doze_fetch::child::{self, Place};
use doze_fetch::mirror::Mirror;
use doze_fetch::modules::Modules;
use std::path::Path;

/// The argument that runs it.
pub const ARG: &str = "__fetch";

/// Serves one request from stdin; the exit code (0 even when the download failed: the reply says why).
pub fn serve(home: &Path, registry: registry::Config) -> u8 {
    let place = Place {
        mirror: Mirror::from_env(home),
        modules: Modules {
            registry,
            home: home.into(),
            triple: crate::platform::triple(),
            protocol: i64::from(doze_module_proto::version::CURRENT.major),
            doze: VERSION.into(),
        },
        // A daemon started for a dry run says so in its environment: nothing it fetches is pinned in doze.lock.
        lock_writes: std::env::var_os(doze_fetch::child::READ_ONLY_LOCK).is_none(),
    };
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout().lock();
    let mut log = std::io::stderr().lock();
    match child::run(&mut input, &mut out, &mut log, &place) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
