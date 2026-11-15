//! `doze lint` (Go cmd/doze/lint.go): checks doze.hcl without touching the daemon or any data, in Go's two passes.
//!
//! The first needs nothing but the files (doze-config's shape pass): block kinds and names, the arguments of app,
//! service and task blocks, variables, references, the dependency graph. The second hands each module its own block,
//! which needs the module itself, so it runs where modules run: the stack's daemon when it is up (its `lint` op, on
//! doze.hcl as it is on disk now), else the same check once in `doze __daemon --lint`, a child that serves nothing
//! and takes its module processes with it. A module type the registry cannot be reached for (and that is not on this
//! machine) is listed as not checked: never a tick for work not done (rule 3, F4), so lint exits 11 (partial), not 0.
//! `--offline` asks for the first pass alone, and gets it with exit 0.
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
    let modules = !unchecked.is_empty();
    let mut what = "module blocks";
    let mut why = match (a.offline, unchecked.is_empty()) {
        (true, false) => "--offline".to_owned(),
        _ => String::new(),
    };
    if !unchecked.is_empty() && !a.offline {
        // The module pass: each module's own check of its block, and the full load's warnings.
        match module_pass(ctx, mode, &loaded.project) {
            Ok(r) if !r.ok => return Err(failed(&r.error)),
            Ok(r) => {
                cfg.warnings = r.lines;
                unchecked = r.unchecked;
                why = r.result;
            }
            Err(could_not) => why = could_not,
        }
    }
    // With no module block the second pass needs nothing either: Go's full load, warnings and all.
    if !modules && !a.offline {
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

/// The module pass's answer: from the stack's daemon when it runs (a Go daemon has no `lint` op), else from a one-shot
/// `doze __daemon --lint`. `Err` says why it could not run at all (then every module block is unchecked).
fn module_pass(
    ctx: &Ctx<'_>,
    mode: &Mode,
    project: &crate::project::Project,
) -> Result<doze_control::Response, String> {
    use doze_control::{Error, Op, Request};
    // The daemon checks with the `--var`s it was started with; this command's own (Go's lint read them) go to the
    // one-shot instead.
    if mode.vars.is_empty()
        && let Ok(stack) = crate::stack::Stack::find(ctx, mode)
        && let Ok(Some(conn)) = stack.open(mode.timeout)
    {
        match conn.call(Request::op(Op::Lint)) {
            Ok(r) => return Ok(r),
            Err(Error::Daemon { message, .. })
                if !message.starts_with("unknown op") && !message.starts_with("this daemon") =>
            {
                return Ok(doze_control::Response {
                    ok: false,
                    error: message,
                    ..doze_control::Response::default()
                });
            }
            // A Go daemon, one that cannot lint, or one that did not answer: the check runs on its own.
            Err(_) => {}
        }
    }
    let exe = ctx
        .host
        .daemon_exe()
        .ok_or("this doze could not find its own program to run the modules with")?;
    let config = ctx
        .cwd
        .join(project.source.as_ref().unwrap_or(&project.config));
    let out = std::process::Command::new(&exe)
        .args([
            doze_daemon::ctl::ARG,
            doze_daemon::entry::LINT_ARG,
            "--config",
        ])
        .arg(&config)
        .current_dir(&ctx.cwd)
        // As `up` hands them to the daemon (`ctl::start`): never on a command line, where `ps` shows them.
        .envs(mode.vars.iter().map(|(k, v)| (format!("DOZE_VAR_{k}"), v)))
        .stdin(std::process::Stdio::null())
        // A person watching sees what takes long (its `__fetch` children say each download); a script gets the answer.
        .stderr(if mode.json || mode.quiet {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::inherit()
        })
        .output()
        .map_err(|e| format!("the module pass could not be run ({e})"))?;
    // The answer is the last line; anything printed before it is not the answer.
    let text = String::from_utf8_lossy(&out.stdout);
    let last = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    serde_json::from_str(last).map_err(|_| {
        let said = String::from_utf8_lossy(&out.stderr);
        let last = said.lines().last().unwrap_or("it said nothing");
        format!("the module pass did not answer ({last})")
    })
}

/// A module pass that found a mistake: exit 5, the load's own words (positions and all) as its lines.
fn failed(error: &str) -> Diagnostic {
    let mut lines = error.lines().filter(|l| !l.trim().is_empty());
    let first = lines
        .next()
        .unwrap_or("doze.hcl does not load with its modules");
    let mut d = Diagnostic::new(ids::CONFIG_INVALID, first.trim().to_owned());
    for l in lines {
        d = d.note(l.trim_end().to_owned());
    }
    d.fix("correct doze.hcl, then doze lint again")
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
