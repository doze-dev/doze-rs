//! `doze lint` (Go cmd/doze/lint.go): checks doze.hcl without touching the daemon or any data, in Go's two passes.
//!
//! The first needs nothing but the files (doze-config's shape pass): block kinds and names, the arguments of app,
//! service and task blocks, variables, references, the dependency graph. The second hands each module its own block,
//! which needs the module itself; this doze runs modules with its daemon (core plan D1), so until then every module
//! block is reported as not checked. Never a tick for work not done (rule 3, F4): with module blocks unchecked, lint
//! exits 11 (partial), not 0. `--offline` asks for the first pass alone, and gets it with exit 0.
//!
//! The JSON result is Go's (`config`, `declared` by kind, `disabled`, `unchecked`, `why`, `warnings`).

use crate::cli::setup::Lint;
use crate::config;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::view::View;
use doze_config::{Config, Kind};
use serde::Serialize;
use std::collections::BTreeMap;

/// Why the module pass did not run, when the person did not ask to skip it.
const NO_MODULE_PASS: &str = "needs this doze's daemon modules, core plan D1; so do the port and network checks across the stack";

/// Go's lint result; the field names are a published contract.
#[derive(Debug, Serialize)]
pub struct Verdict {
    config: String,
    declared: BTreeMap<&'static str, usize>,
    disabled: usize,
    unchecked: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    why: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    /// What `unchecked` lists, for the human line: module blocks, or certificates.
    #[serde(skip)]
    what: &'static str,
}

/// `doze lint [--offline]`.
pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Lint) -> Outcome {
    let loaded = config::shape(ctx, mode)?;
    let mut cfg = loaded.config;
    let mut unchecked = module_types(&cfg);
    let mut what = "module blocks";
    let mut why = match (a.offline, unchecked.is_empty()) {
        (_, true) => String::new(),
        (true, false) => "--offline".to_owned(),
        (false, false) => NO_MODULE_PASS.to_owned(),
    };
    // With no module block the second pass needs nothing either: Go's full load, warnings and all.
    if unchecked.is_empty() && !a.offline {
        let (full, certs_skipped) =
            config::Loader::new(ctx, mode).full_without_modules(&loaded.project)?;
        cfg = full;
        if certs_skipped {
            unchecked.push("certificates".to_owned());
            what = "certificates";
            why = "this doze reads a certificate when it serves it".to_owned();
        }
    }
    // As typed, relative to here (`-c .` is `.`, as Go prints it).
    let shown = crate::project::shown(
        loaded
            .project
            .source
            .as_ref()
            .unwrap_or(&loaded.project.config),
        &ctx.cwd,
    );
    let problem = (!a.offline && !unchecked.is_empty()).then(|| {
        Diagnostic::new(
            ids::NOT_ALL_CHECKED,
            "lint is partial: not every block was checked",
        )
        .fix("--offline runs the first pass alone and accepts it (exit 0)")
    });
    let verdict = Verdict {
        config: shown,
        declared: declared(&cfg),
        disabled: cfg.instances.iter().filter(|i| !i.enabled).count(),
        unchecked,
        why,
        warnings: cfg.warnings.clone(),
        what,
    };
    Ok(Done {
        view: View::Lint(verdict),
        problem,
    })
}

/// The module types doze.hcl declares, each once, sorted (Go `moduleTypes`).
fn module_types(cfg: &Config) -> Vec<String> {
    let mut out: Vec<String> = cfg
        .instances
        .iter()
        .filter(|i| i.kind() == Kind::Module)
        .map(|i| i.ty.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// How many blocks of each kind (Go counts services and tasks from their own lists).
fn declared(cfg: &Config) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for i in &cfg.instances {
        *out.entry(i.kind().as_str()).or_insert(0) += 1;
    }
    for (kind, n) in [
        (Kind::Service, cfg.services.len()),
        (Kind::Task, cfg.tasks.len()),
    ] {
        out.insert(kind.as_str(), n);
    }
    // Go always reports services and tasks, 0 included; the human line leaves zeros out.
    out
}

impl Verdict {
    /// Go's lines: the verdict, what was not checked and why, then each warning.
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut parts: Vec<String> = Vec::new();
        for kind in Kind::ALL {
            if let Some(n) = self.declared.get(kind.as_str()).filter(|n| **n > 0) {
                parts.push(plural(*n, kind.as_str()));
            }
        }
        let mut summary = if parts.is_empty() {
            "nothing declared".to_owned()
        } else {
            parts.join(", ")
        };
        if self.disabled > 0 {
            summary.push_str(&format!(" ({} disabled)", self.disabled));
        }
        let mut out = if self.unchecked.is_empty() {
            format!("{} {} is valid: {summary}\n", p.ok("✓"), self.config)
        } else {
            // Rule 3: no tick for a check that did not run.
            format!(
                "{} {} passes the first pass: {summary}\n  {} {} not checked ({}): {}\n",
                p.ok("✓"),
                self.config,
                p.warn("!"),
                self.what,
                self.why,
                self.unchecked.join(", ")
            )
        };
        for w in &self.warnings {
            out.push_str(&format!("  {} {w}\n", p.warn("!")));
        }
        out
    }
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
