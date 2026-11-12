//! What every Go module wrote for itself (module audit, "shared pattern"): resolving a version against the lock and
//! the host's mirror, settings pass-through, running the engine's own tools, clearing stale files.

use crate::module::{Cx, Result};
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::instance::{Address, Instance, Toolchain};
use doze_module_proto::msg::run::{Pin, Provisioned, Resolved};
use std::collections::BTreeMap;
use std::process::Stdio;

/// Whether `version` names an exact build: at least `dots` dots ("8.1.8" with 2; "16.14" with 1).
pub fn is_exact(version: &str, dots: u8) -> bool {
    version.matches('.').count() >= usize::from(dots)
}

/// The usual `resolve`: `DOZE_<ENGINE>_BINDIR` if set (a developer's own build), else the lock's pin, else an exact
/// version as given, else the major through the host's mirror; then the host makes the build present and verified,
/// and the pin is returned for the lock.
pub async fn resolve_via<S>(
    cx: &Cx<S>,
    pins: &[Pin],
    target: &str,
    exact_dots: u8,
) -> Result<Resolved> {
    let engine = cx.instance.engine.clone();
    let spec = cx.instance.version.clone();
    let env = format!(
        "DOZE_{}_BINDIR",
        engine.to_ascii_uppercase().replace('-', "_")
    );
    if let Some(dir) = std::env::var_os(&env) {
        let toolchain = Toolchain {
            engine,
            full: spec,
            bin_dir: dir.to_string_lossy().into_owned(),
            tools: BTreeMap::new(),
        };
        return Ok(Resolved {
            toolchain,
            pins: Vec::new(),
        });
    }
    let pin = pins.iter().find(|p| p.engine == engine && p.spec == spec);
    let full = match pin {
        Some(p) => p.resolved.clone(),
        None if is_exact(&spec, exact_dots) => spec.clone(),
        None => cx.fetch().resolve_major(&engine, &spec).await?,
    };
    let expected = pin.and_then(|p| p.hashes.get(target)).map(String::as_str);
    let got = cx.fetch().ensure(&engine, &full, target, expected).await?;
    let hashes = BTreeMap::from([(target.to_string(), got.digest)]);
    let pin = Pin {
        engine: engine.clone(),
        spec,
        resolved: full.clone(),
        hashes,
    };
    Ok(Resolved {
        toolchain: Toolchain {
            engine,
            full,
            bin_dir: got.bin_dir,
            tools: BTreeMap::new(),
        },
        pins: vec![pin],
    })
}

/// The user's `settings`, sorted, without the keys doze owns (compared without case: Go module bug 11 compared one way
/// in one place and another way in the other).
pub fn settings<'a>(
    user: &'a BTreeMap<String, String>,
    locked: &[&str],
) -> Vec<(&'a str, &'a str)> {
    user.iter()
        .filter(|(k, _)| !locked.iter().any(|l| l.eq_ignore_ascii_case(k)))
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

/// Runs one of the engine's own tools (no shell, null stdin) and returns its stdout; a failure carries its trimmed
/// stderr. Never use it for a tool that leaves a server behind: that server would inherit the pipes (W0).
pub async fn run_tool(program: &std::path::Path, args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| {
            Error::new(
                ErrorKind::Unavailable,
                format!("{}: {e}", program.display()),
            )
        })?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let why = String::from_utf8_lossy(&out.stderr);
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Err(Error::new(
        ErrorKind::Unavailable,
        format!("{name}: {}", why.trim()),
    ))
}

/// A diagnostic at `path` unless `value` is one of `allowed`, naming the nearest one ("did you mean") or listing them
/// all: the engine would refuse it anyway, but only at boot, in its own log, with no line in doze.hcl to point at.
pub fn one_of(
    path: Vec<doze_module_proto::msg::value::PathSeg>,
    what: &str,
    value: &str,
    allowed: &[&str],
) -> Option<doze_module_proto::msg::error::Diagnostic> {
    if allowed.contains(&value) {
        return None;
    }
    let hint = match crate::spec::closest(value, allowed) {
        Some(near) => format!("did you mean {near:?}?"),
        None => format!("one of: {}", allowed.join(", ")),
    };
    Some(doze_module_proto::msg::error::Diagnostic {
        path,
        message: format!("{value:?} is not {what}"),
        hint: Some(hint),
    })
}

/// An I/O failure as a module error: what was being done to which path, and why it failed.
pub fn io_error(what: &str, path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Error {
    let what = format!("{what} {}", path.display());
    move |e| Error::new(ErrorKind::Unavailable, format!("{what}: {e}"))
}

/// `provision` for an engine that needs only an empty data directory (valkey, kvrocks): made open to this user only,
/// fresh the first time and existing after.
pub fn empty_data_dir(data_dir: &str) -> Result<Provisioned> {
    let dir = std::path::Path::new(data_dir);
    if dir.is_dir() {
        return Ok(Provisioned::Existing);
    }
    doze_os::fs::private_dir(dir).map_err(io_error("creating", dir))?;
    Ok(Provisioned::Fresh)
}

/// Where the engine must listen, made ready to listen on: the main listener's backend as the host granted it (PLAN.md
/// D13), or `fallback` in the instance's socket directory if none was granted. For a socket, its directory is made
/// private and a stale socket from a crash is removed.
pub fn prepare_backend(instance: &Instance, fallback: &str) -> Result<Address> {
    let granted = instance
        .listeners
        .get(doze_module_proto::network::MAIN)
        .map(|l| l.backend.clone());
    let addr = granted.unwrap_or_else(|| {
        let p = std::path::Path::new(&instance.socket_dir).join(fallback);
        Address::Unix(p.to_string_lossy().into_owned())
    });
    if let Address::Unix(p) = &addr {
        let socket = std::path::Path::new(p);
        if let Some(dir) = socket.parent() {
            doze_os::fs::private_dir(dir).map_err(io_error("creating", dir))?;
        }
        clear_stale(socket)?;
    }
    Ok(addr)
}

/// A connection to the running engine's main backend, for a module that asks its engine something (an admin
/// action, console numbers). `Unavailable` if it is not running: the console then shows the instance asleep.
pub async fn dial_backend(instance: &Instance) -> Result<doze_os::net::Stream> {
    let Some(granted) = instance.listeners.get(doze_module_proto::network::MAIN) else {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "the instance has no backend granted",
        ));
    };
    let dialed = match &granted.backend {
        Address::Unix(p) => doze_os::net::connect_unix(p).await,
        Address::Tcp(a) => doze_os::net::connect_tcp(a).await,
    };
    dialed.map_err(|e| {
        Error::new(
            ErrorKind::Unavailable,
            format!("the engine is not answering: {e}"),
        )
    })
}

/// The bytes the files under `dir` hold, for an engine whose own figures for its disk use are missing or misleading
/// (kvrocks reports the whole disk). Links are not followed; what cannot be read counts as nothing.
pub fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// A loopback TCP address split into host and port, for engines that take them separately.
pub fn host_port(addr: &str) -> Result<(&str, &str)> {
    addr.rsplit_once(':')
        .filter(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok())
        .ok_or_else(|| Error::new(ErrorKind::Invalid, format!("{addr:?} is not host:port")))
}

/// Removes a stale socket or pid file left by an engine that did not exit cleanly; absent is fine.
pub fn clear_stale(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::new(
            ErrorKind::Unavailable,
            format!("removing {}: {e}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_settings_are_dropped_whatever_their_case() {
        let user = BTreeMap::from([
            ("Port".to_string(), "1".to_string()),
            ("work_mem".to_string(), "64MB".to_string()),
        ]);
        assert_eq!(settings(&user, &["port"]), vec![("work_mem", "64MB")]);
        assert!(is_exact("8.1.8", 2) && !is_exact("8.1", 2) && is_exact("16.14", 1));
    }
}
