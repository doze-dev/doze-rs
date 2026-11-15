//! `lint` (additive; Go's `doze lint` second pass, which loaded doze.hcl with its modules in the CLI): the stack's
//! doze.hcl, as it is on disk now, loaded through this daemon's module catalog, so each module checks its own block.
//! The CLI does the first pass (the shape) itself and asks for this one.
//!
//! The answer: `ok` with the config's warnings as `lines` when every block passes; `unchecked` lists the module types
//! whose build could not be had (`{"type", "why"}`: the registry unreachable, a download or signature that failed),
//! still `ok` (Go's lint passed on the first pass alone then); otherwise the load's error, positions and all. Nothing
//! starts; a module type or major the stack had not used yet is fetched and pinned in doze.lock, as `up` would.

use crate::Daemon;
use crate::catalog::Catalog;
use doze_control::Response;
use std::collections::BTreeMap;

fn failed(error: String) -> Response {
    Response {
        ok: false,
        error,
        ..Response::default()
    }
}

/// The `lint` op. `path` is the doze.hcl to check; empty means this daemon's own, and another stack's is refused (its
/// lock and modules are not this daemon's).
pub(crate) async fn lint(d: &Daemon, path: &str) -> Response {
    let Some(live) = d.inner.live.as_ref() else {
        return failed("this daemon cannot lint".into());
    };
    let own = live.config();
    if !path.is_empty() && !same_file(path, own) {
        return failed(format!(
            "this daemon serves {own}; lint {path} with its own stack's daemon, or without one"
        ));
    }
    let owned = d.clone();
    let done = tokio::task::spawn_blocking(move || {
        owned
            .inner
            .live
            .as_ref()
            .map(|l| check(l.catalog(), l.config(), l.version()))
    })
    .await;
    match done {
        Ok(Some(r)) => r,
        Ok(None) => failed("this daemon cannot lint".into()),
        Err(err) => failed(err.to_string()),
    }
}

fn same_file(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `doze __daemon --lint`: [`once`], its answer as one line on stdout (the whole answer; the CLI reads it).
pub(crate) fn main(
    rt: &tokio::runtime::Runtime,
    config: &std::path::Path,
    version: &str,
) -> std::process::ExitCode {
    let answer = rt.block_on(once(config, version));
    #[allow(clippy::print_stdout)]
    {
        println!("{}", serde_json::to_string(&answer).unwrap_or_default());
    }
    std::process::ExitCode::SUCCESS
}

/// The `lint` op's module pass on `config`, with no daemon: the catalog it needs (builds through `__fetch`, the lock
/// beside doze.hcl), the check, then the catalog's module processes stopped before the answer is given.
pub(crate) async fn once(config: &std::path::Path, version: &str) -> doze_control::Response {
    let failed = |error: String| doze_control::Response {
        ok: false,
        error,
        ..doze_control::Response::default()
    };
    let catalog = match crate::entry::catalog(config) {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => return failed(e),
    };
    let (cat, path, version) = (
        catalog.clone(),
        config.to_string_lossy().into_owned(),
        version.to_owned(),
    );
    let answer = tokio::task::spawn_blocking(move || check(&cat, &path, &version))
        .await
        .unwrap_or_else(|e| failed(e.to_string()));
    catalog.pool.stop_all().await;
    answer
}

/// Loads `config` through `cat`: the module pass.
pub fn check(cat: &Catalog, config: &str, version: &str) -> Response {
    cat.retry_failed();
    let loaded = crate::entry::with_options(version, cat, Default::default(), |o| {
        doze_config::load(config, o)
    });
    match loaded {
        Ok(cfg) => Response {
            ok: true,
            lines: cfg.warnings,
            ..Response::default()
        },
        Err(err) => {
            let unchecked = unreachable_types(cat);
            if unchecked.is_empty() {
                return failed(err);
            }
            Response {
                ok: true,
                unchecked: unchecked
                    .into_iter()
                    .map(|(ty, why)| doze_control::UncheckedView { ty, why })
                    .collect(),
                ..Response::default()
            }
        }
    }
}

/// The module types whose build could not be had in this load, with why: the registry unreachable, a download or a
/// signature that failed. Empty when a type is not published at all (a mistake in doze.hcl, said as the load's error)
/// or when every build was found (the error is then the config's). Read from the catalog, not from doze.hcl: a block
/// whose module is missing leaves its references unknown, and a second read would stop at those.
fn unreachable_types(cat: &Catalog) -> BTreeMap<String, String> {
    let failed = cat.failures();
    if failed.values().any(|why| why.contains("is not published")) {
        return BTreeMap::new();
    }
    failed
}
