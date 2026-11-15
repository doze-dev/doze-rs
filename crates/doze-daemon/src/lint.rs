//! `lint` (additive; Go's `doze lint` second pass, which loaded doze.hcl with its modules in the CLI): the stack's
//! doze.hcl, as it is on disk now, loaded through this daemon's module catalog, so each module checks its own block.
//! The CLI does the first pass (the shape) itself and asks for this one.
//!
//! The answer: `ok` with the config's warnings as `lines` when every block passes; `unchecked` names the module types
//! the pass could not check because the registry cannot be reached and they were not on this machine (Go's lint
//! passed on the first pass alone then); otherwise the load's error, positions and all. Nothing starts; a module type
//! or major the stack had not used yet is fetched and pinned in doze.lock, as `up` would.

use crate::Daemon;
use crate::catalog::Catalog;
use doze_config::modules::Modules;
use doze_control::Response;
use std::collections::BTreeSet;

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
            let unchecked = unreachable_types(cat, config, version);
            if unchecked.is_empty() {
                return failed(err);
            }
            Response {
                ok: true,
                unchecked: unchecked.into_iter().collect(),
                result: "not cached, and the registry cannot be reached".into(),
                ..Response::default()
            }
        }
    }
}

/// The module types of `config` (read without modules) whose build could not be found because the registry cannot be
/// reached; empty when any type failed for another reason (a type that does not exist is an error, not unchecked).
fn unreachable_types(cat: &Catalog, config: &str, version: &str) -> BTreeSet<String> {
    let Ok(shape) = crate::entry::with_options(version, cat, Default::default(), |o| {
        doze_config::load_shape(config, o)
    }) else {
        return BTreeSet::new();
    };
    let mut out = BTreeSet::new();
    for i in shape
        .instances
        .iter()
        .filter(|i| i.kind() == doze_config::Kind::Module)
    {
        match cat.lookup_error(&i.ty) {
            Some(why) if why.contains("can't be reached") || why.contains("cannot be reached") => {
                out.insert(i.ty.clone());
            }
            Some(_) => return BTreeSet::new(),
            None => {}
        }
    }
    out
}
