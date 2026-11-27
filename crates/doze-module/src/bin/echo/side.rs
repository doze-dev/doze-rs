//! The echo's second listener, for the daemon's tests of a module that declares more than `main` (temporal's `ui`,
//! kafka's `registry`): with the setting `side` (a port) the module declares a TCP listener `side` there and runs a
//! second engine on what the host granted for it; a copy of this program named `…-supervised` is supervised, so
//! nothing fronts it.

use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Address, Instance, Listener};
use doze_module::proto::msg::run::{Process, Ready, StopHow};
use std::collections::BTreeMap;

/// The listener's name.
pub(crate) const SIDE: &str = "side";

/// Whether this copy of the module is the supervised one (its file is named `…-supervised`).
pub(crate) fn supervised() -> bool {
    named("-supervised")
}

/// Whether this copy of the program's file name ends with `suffix` (a copy the daemon's tests made to vary it).
pub(crate) fn named(suffix: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().ends_with(suffix)))
        .unwrap_or(false)
}

/// The listeners declared: `main`, and `side` at the setting's port when there is one.
pub(crate) fn network(settings: &BTreeMap<String, String>) -> Vec<Listener> {
    let main = Listener {
        name: "main".into(),
        port: 0,
        http: false,
        routes: Vec::new(),
        tcp: false,
    };
    let side = settings.get(SIDE).map(|p| Listener {
        name: SIDE.into(),
        port: p.parse().unwrap_or(0),
        http: false,
        routes: Vec::new(),
        tcp: true,
    });
    std::iter::once(main).chain(side).collect()
}

/// What it publishes of `side`: its address, as `ECHO_SIDE`.
pub(crate) fn env(
    settings: &BTreeMap<String, String>,
    instance: &Instance,
) -> Option<(String, String)> {
    settings.get(SIDE)?;
    let addr = instance.listeners.get(SIDE).map(|l| l.addr.clone())?;
    Some(("ECHO_SIDE".into(), addr))
}

/// The second engine, listening where `side` was granted; an error when it was declared and not granted, as temporal
/// refuses to boot without its `ui`.
pub(crate) fn process(
    settings: &BTreeMap<String, String>,
    instance: &Instance,
    program: &str,
) -> doze_module::Result<Option<Process>> {
    if !settings.contains_key(SIDE) {
        return Ok(None);
    }
    let addr = match instance.listeners.get(SIDE).map(|g| &g.backend) {
        Some(Address::Tcp(a)) => a.clone(),
        Some(Address::Unix(_)) => {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "listener \"side\" needs a TCP backend",
            ));
        }
        None => {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "no backend granted for \"side\"",
            ));
        }
    };
    Ok(Some(Process {
        name: SIDE.into(),
        program: program.into(),
        args: vec!["--listen".into(), format!("tcp:{addr}")],
        env: BTreeMap::new(),
        dir: None,
        after: vec![],
        ready: Some(Ready::Tcp(addr)),
        hooks: vec![],
        stop: Some(StopHow::Terminate),
    }))
}
