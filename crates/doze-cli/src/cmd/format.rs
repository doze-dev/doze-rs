//! `doze fmt` (Go cmd/doze/fmt.go): rewrites the config's files in HCL's canonical layout, byte for byte what Go's
//! `hclwrite.Format` writes (core plan O11, doze-hcl H3). It changes layout only, and leaves a file that does not
//! parse untouched (exit 5, every syntax error drawn). It prints each file it changed.
//!
//! `--check` writes nothing, lists the files that would change and exits 1 if there are any (CI). `--dry-run` writes
//! nothing and shows the change itself, a unified diff per file.

use crate::cli::setup::Fmt;
use crate::cmd::modules;
use crate::config::{Loader, render};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::view::View;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// What `fmt` did, or would do.
#[derive(Debug, Serialize)]
pub struct Formatted {
    /// The files whose layout changes, as the person would type them.
    files: Vec<String>,
    /// Whether they were written (not with `--check` or `--dry-run`).
    written: bool,
    /// The diffs, with `--dry-run`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diffs: Vec<String>,
}

struct Change {
    path: PathBuf,
    shown: String,
    before: Vec<u8>,
    after: Vec<u8>,
}

/// `doze fmt [--check] [--dry-run]`.
pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Fmt) -> Outcome {
    let project = modules::find(ctx, mode)?;
    let loader = Loader::new(ctx, mode);
    let mut changes = Vec::new();
    for path in loader.files(&project)? {
        if let Some(c) = change(&path, loader.cwd())? {
            changes.push(c);
        }
    }
    let write = !a.check && !a.dry.dry_run;
    if write {
        for c in &changes {
            // Writing over the file keeps its permissions, as Go keeps them.
            std::fs::write(&c.path, &c.after).map_err(|e| {
                Diagnostic::new(
                    ids::CONFIG_UNWRITABLE,
                    format!("{} can't be written: {e}", c.shown),
                )
            })?;
        }
    }
    let diffs = if a.dry.dry_run {
        changes
            .iter()
            .map(|c| {
                let (before, after) = (
                    String::from_utf8_lossy(&c.before),
                    String::from_utf8_lossy(&c.after),
                );
                crate::diff::unified(&c.shown, &before, &after)
            })
            .collect()
    } else {
        Vec::new()
    };
    let problem = (a.check && !changes.is_empty()).then(|| {
        Diagnostic::new(
            ids::NOT_FORMATTED,
            format!("{} not formatted", count(changes.len())),
        )
        .fix("doze fmt")
    });
    Ok(Done {
        view: View::Fmt(Formatted {
            files: changes.into_iter().map(|c| c.shown).collect(),
            written: write,
            diffs,
        }),
        problem,
    })
}

/// The file's new layout, when it differs; exit 5 when it does not parse.
fn change(path: &Path, cwd: &Path) -> Result<Option<Change>, Diagnostic> {
    let shown = shown(path, cwd);
    let before = std::fs::read(path).map_err(|e| {
        Diagnostic::new(
            ids::CONFIG_UNREADABLE,
            format!("{shown} can't be read: {e}"),
        )
    })?;
    let diags = doze_hcl::check::check(&before, &path.display().to_string());
    if diags.has_errors() {
        return Err(render::unparsable(path, &diags, cwd));
    }
    let after = doze_hcl::fmt::format(&before);
    Ok((after != before).then(|| Change {
        path: path.to_path_buf(),
        shown,
        before,
        after,
    }))
}

/// A path relative to where the person is, when it is under there (Go's `displayPath`).
fn shown(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .map_or_else(|_| path.display().to_string(), |r| r.display().to_string())
}

fn count(n: usize) -> String {
    if n == 1 {
        "1 file is".into()
    } else {
        format!("{n} files are")
    }
}

impl Formatted {
    /// Go's lines: each changed file; with `--dry-run`, the diffs and what applies them.
    pub(crate) fn human(&self, p: Paint) -> String {
        if !self.diffs.is_empty() {
            let mut out: String = self.diffs.concat();
            out.push_str(&p.dim("  dry run: nothing was changed · doze fmt applies this"));
            out.push('\n');
            return out;
        }
        self.files.iter().map(|f| format!("{f}\n")).collect()
    }
}
