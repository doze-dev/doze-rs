//! What every Go module wrote for itself (module audit, "shared pattern"): resolving a version against the lock and
//! the host's mirror, settings pass-through, running the engine's own tools, clearing stale files.

use crate::module::{Cx, Result};
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::instance::Toolchain;
use doze_module_proto::msg::run::{Pin, Resolved};
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
