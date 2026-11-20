//! What modules declare, for `doze add` and `doze init`: each module's `meta.yaml` from the registry through
//! doze-fetch (its block label, port, default version, category and how a project shows it uses it), with the signed
//! index for the engine versions when the module names no default. This is the CLI's only source of module
//! knowledge: there is no table of engines here (owner rule: modules declare).
//!
//! Rule 5: this machine's copy is used when there is one (refreshed in the background when a day old); a module never
//! seen goes out once, within the registry budget. Offline, the copies are what there is, and with none the command
//! says so.

use super::fetch::{self, Prefer};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::registry::Cache;
use crate::registry::select::compare_versions;
use crate::registry::source::Source;
use doze_detect::Declared;

/// The namespace whose modules `init` guesses from (the official one).
const NS: &str = "doze";

/// What `name` declares: `Ok(None)` when the registry says there is no such module (so it is a service), an error
/// when that cannot be known (the registry unreachable and no copy here, a refused index).
pub(crate) fn one(ctx: &mut Ctx<'_>, name: &str) -> Result<Option<Declared>, Diagnostic> {
    let Some(src) = Source::parse(name) else {
        return Ok(None);
    };
    match fetch::module(ctx, &src, true, Prefer::Cache) {
        Ok(m) => Ok(Some(declared(&src, &m))),
        Err(d) if d.id == ids::NO_SUCH_MODULE => Ok(None),
        Err(d) => Err(d),
    }
}

/// The module's declaration, from its documentation and its signed index.
fn declared(src: &Source, m: &fetch::Module) -> Declared {
    let meta = m.meta.clone().unwrap_or_default();
    let newest_engine = || {
        m.index
            .stable()
            .and_then(|v| m.index.releases.get(v))
            .and_then(|r| {
                r.engines
                    .iter()
                    .max_by(|a, b| compare_versions(a, b))
                    .cloned()
            })
            .unwrap_or_default()
    };
    Declared {
        module: src.name().to_owned(),
        label: if meta.label.is_empty() {
            src.name().to_owned()
        } else {
            meta.label.clone()
        },
        port: meta.port.unwrap_or(0),
        version: if meta.default_version.is_empty() {
            newest_engine()
        } else {
            meta.default_version.clone()
        },
        category: meta.category.clone(),
        detect: meta.detect.clone().unwrap_or_default(),
        about: meta.tagline.clone(),
    }
}

/// Every official module's declaration `init` can have: this machine's copies and the registry's catalog. Also
/// says what could not be had (rule 3: a guess made without some modules says so).
pub(crate) struct Known {
    pub(crate) declared: Vec<Declared>,
    /// Why some modules were not consulted; empty when every one was.
    pub(crate) missing: String,
    /// No module could be consulted at all, so none could be guessed (the run is partial, exit 11).
    pub(crate) none: bool,
}

pub(crate) fn all(ctx: &mut Ctx<'_>) -> Known {
    let mut names = Cache::new(&ctx.home).modules(NS);
    let catalog = ctx.registry.get("index.json");
    let offline = match &catalog {
        Ok(got) => {
            names.extend(catalog_names(&got.body));
            None
        }
        Err(e) => Some(e.reasons()),
    };
    names.sort();
    names.dedup();
    let mut declared = Vec::new();
    let mut failed = Vec::new();
    for name in names {
        match Source::parse(&format!("{NS}/{name}")) {
            Some(src) => match fetch::module(ctx, &src, true, Prefer::Cache) {
                Ok(m) => declared.push(self::declared(&src, &m)),
                Err(_) => failed.push(name),
            },
            None => failed.push(name),
        }
    }
    let missing = match (offline, declared.is_empty()) {
        (Some(why), true) => format!(
            "no module could be guessed: the registry can't be reached ({why}) and this machine has no module's declaration"
        ),
        (Some(why), false) => format!(
            "modules were guessed from this machine's copies only ({}): the registry can't be reached ({why})",
            declared
                .iter()
                .map(|d| d.module.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        (None, _) if !failed.is_empty() => {
            format!(
                "not consulted (their declarations could not be read): {}",
                failed.join(", ")
            )
        }
        (None, _) => String::new(),
    };
    let none = declared.is_empty() && !missing.is_empty();
    Known {
        declared,
        missing,
        none,
    }
}

/// The module names the registry's catalog lists for the official namespace.
fn catalog_names(body: &[u8]) -> Vec<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer(&format!("/namespaces/{NS}/modules"))
                .and_then(|m| m.as_object())
                .map(|m| m.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// The modules this machine or the registry knows by name, for "did you mean" (no network: the catalog only when
/// it is already in hand).
pub(crate) fn names_here(ctx: &Ctx<'_>) -> Vec<String> {
    Cache::new(&ctx.home).modules(NS)
}
