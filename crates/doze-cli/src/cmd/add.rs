//! `doze add <module|service> [name]` (Go cmd/doze/add.go): writes one block into doze.hcl beside what is there,
//! keeping its comments and order (doze-hcl's editor, byte for byte Go's hclwrite), and shows the block and the line
//! that wires it into an app.
//!
//! A module's block comes from what the module declares (its label, port and default version, from the registry's
//! `meta.yaml`); a name the registry has no module for is a `service`, run from a binary on the PATH, a container
//! or a download (`--as`). Nothing is written unless the result reads as a config. `--dry-run` writes nothing.
//!
//! Better than Go: a name one letter from a module or a known service is not silently added as a service (it says
//! "did you mean", exit 4); the block of a service whose name was taken is labelled with the free name, not the taken
//! one; the registry unreachable with no copy here is said as that (exit 10), not guessed from a built-in table.

use super::decl;
use crate::cli::setup::Add;
use crate::config::Loader;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::view::View;
use doze_detect::{Declared, services};
use doze_hcl::edit::Edit;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// What `add` wrote, or would write.
#[derive(Debug, Serialize)]
pub struct Added {
    /// The config file, as the person would type it.
    pub file: String,
    /// `module` or `service`.
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub module: String,
    pub name: String,
    /// The block, as written.
    pub block: String,
    /// The line that wires it into an app, `ENV = reference`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wiring: Option<Wiring>,
    pub written: bool,
    /// For a service: how it will be run, and the other ways.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub how: Vec<String>,
}

/// An app's variable and what answers it.
#[derive(Debug, Serialize)]
pub struct Wiring {
    pub env: String,
    pub reference: String,
}

/// The config `add` edits: the project's, or `doze.hcl` here when there is none (then it is new).
pub(crate) fn target(ctx: &Ctx<'_>, mode: &Mode) -> PathBuf {
    match crate::project::find(&ctx.cwd, mode.config.as_deref()) {
        Some(p) => p.config,
        None => ctx.cwd.join(
            mode.config
                .as_deref()
                .unwrap_or(Path::new(crate::project::CONFIG)),
        ),
    }
}

/// The file's text, or `None` when there is no file yet.
pub(crate) fn read(path: &Path, shown: &str) -> Result<Option<String>, Diagnostic> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Diagnostic::new(
            ids::CONFIG_UNREADABLE,
            format!("{shown} can't be read: {e}"),
        )),
    }
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Add) -> Outcome {
    let path = target(ctx, mode);
    let shown = crate::project::shown(&path, &ctx.cwd);
    let existing = read(&path, &shown)?;
    let mut ed = Edit::open(existing.as_deref().unwrap_or_default())
        .map_err(|d| crate::config::render::unparsable(&path, &d, &ctx.cwd))?;
    let kind = a.what.to_lowercase();
    let module = lookup(ctx, &kind, a.run_as.is_some())?;
    let mut added = match &module {
        Some(_) if a.run_as.is_some() => {
            return Err(Diagnostic::new(
                ids::WAY_REFUSED,
                format!("--as is for a service, and {kind} is a registry module"),
            )
            .fix(format!("doze add {kind}")));
        }
        Some(d) => module_block(&ed, d, a),
        None => {
            refuse_near_miss(ctx, &kind, a)?;
            service_block(ctx, &ed, &kind, a)?
        }
    };
    if declared(&ed, &added.name) {
        return Err(Diagnostic::new(
            ids::NAME_TAKEN,
            format!("\"{}\" is already declared in {shown}", added.name),
        )
        .fix(format!("pick another name: doze add {kind} <name>")));
    }
    if existing.is_none() {
        let dir = path.parent().unwrap_or(&ctx.cwd);
        let base = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        let slug = doze_detect::slug(
            &base
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default(),
        );
        ed.set_top_attribute("stack", if slug.is_empty() { "stack" } else { &slug });
    }
    ed.append_block(&added.block).map_err(|e| {
        Diagnostic::new(
            ids::BUG,
            format!("the block doze made does not parse ({e:?})"),
        )
        .fix("doze bug")
    })?;
    let out = ed.bytes();
    must_read(ctx, mode, &path, &out, &shown)?;
    added.block =
        String::from_utf8_lossy(&doze_hcl::fmt::format(added.block.as_bytes())).into_owned();
    added.file = shown;
    if !a.dry.dry_run {
        write(&path, &out, &added.file)?;
        added.written = true;
    }
    Ok(Done::ok(View::Added(added)))
}

/// What `kind` declares as a module; `None` for a service. With `--as`, an unreachable registry is taken as the
/// person's word that it is a service.
fn lookup(
    ctx: &mut Ctx<'_>,
    kind: &str,
    service_asked: bool,
) -> Result<Option<Declared>, Diagnostic> {
    match decl::one(ctx, kind) {
        Err(d) if service_asked && d.id == ids::REGISTRY_UNREACHABLE => Ok(None),
        Err(d) if d.id == ids::REGISTRY_UNREACHABLE => Err(d
            .note(format!("doze can't tell whether {kind} is a module or a service"))
            .fix(format!(
                "check the network and run it again · doze add {kind} --as container adds it as a service"
            ))),
        other => other,
    }
}

/// A name the registry has no module for, one slip from a module or service doze knows, is a typo, not a service.
fn refuse_near_miss(ctx: &Ctx<'_>, kind: &str, a: &Add) -> Result<(), Diagnostic> {
    if a.run_as.is_some() || services::recipe(kind).is_some() {
        return Ok(());
    }
    let mut known = decl::names_here(ctx);
    known.extend(services::names());
    let Some(near) = crate::suggest::nearest(kind, &known).filter(|n| *n != kind) else {
        return Ok(());
    };
    Err(Diagnostic::new(
        ids::NO_SUCH_MODULE,
        format!("there is no module called {kind}, and doze knows no service by that name"),
    )
    .note(format!("did you mean {near}?"))
    .fix(format!(
        "doze add {near} · to add {kind} as a service anyway: doze add {kind} --as container"
    )))
}

fn module_block(ed: &Edit, d: &Declared, a: &Add) -> Added {
    let name = a.name.clone().unwrap_or_else(|| free_name(ed, &d.label));
    let version = a.version.clone().unwrap_or_else(|| d.version.clone());
    let port = a.port.unwrap_or(d.port);
    Added {
        file: String::new(),
        kind: "module".into(),
        module: d.module.clone(),
        block: doze_detect::render_module(&d.module, &name, &version, port),
        wiring: doze_detect::wiring(d).map(|(env, attr)| Wiring {
            env,
            reference: format!("module.{name}.{attr}"),
        }),
        name,
        written: false,
        how: Vec::new(),
    }
}

/// Go's `serviceBlockFor`: the three ways to bring something that is not a module, and which one was taken.
fn service_block(ctx: &Ctx<'_>, ed: &Edit, kind: &str, a: &Add) -> Result<Added, Diagnostic> {
    let name = a
        .name
        .clone()
        .unwrap_or_else(|| free_name(ed, &kind.replace([' ', '_'], "-")));
    let recipe = services::recipe(kind);
    let port = a.port.unwrap_or_else(|| recipe.map_or(0, |r| r.port));
    let binary = services::binary(kind);
    let found = on_path(ctx, binary)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let mut how = Vec::new();
    let block = match a.run_as.as_deref() {
        None => {
            how.push(format!(
                "{kind} is not a doze module — adding it as a service"
            ));
            how.push(match (found.is_empty(), recipe.is_some()) {
                (false, true) => format!("found {binary} at {found} — the block runs it"),
                (false, false) => format!(
                    "found {binary} at {found}, but doze does not know its flags — edit `run`"
                ),
                (true, true) => {
                    format!("{binary} is not installed — the block runs it as a container")
                }
                (true, false) => {
                    "nothing found on this machine — the block is a download to fill in".into()
                }
            });
            doze_detect::service_block(&name, kind, port, &found, "")
        }
        Some("installed") if found.is_empty() => {
            return Err(Diagnostic::new(ids::WAY_REFUSED, format!("{binary} is not on your PATH"))
                .fix(format!("install it, or use --as container or --as download: doze add {kind} --as container")));
        }
        Some("installed") => doze_detect::service_block(&name, kind, port, &found, ""),
        Some("container") => {
            doze_detect::service_block(&name, kind, port, "", services::image(kind))
        }
        Some(_) => doze_detect::download_block(&name, kind, port),
    };
    Ok(Added {
        file: String::new(),
        kind: "service".into(),
        module: String::new(),
        block,
        wiring: Some(Wiring {
            env: format!("{}_ADDR", name.replace('-', "_").to_uppercase()),
            reference: format!("service.{name}.address"),
        }),
        name,
        written: false,
        how,
    })
}

/// A program on the PATH the run sees.
fn on_path(ctx: &Ctx<'_>, binary: &str) -> Option<PathBuf> {
    let path = ctx.var("PATH")?;
    let ext = ctx.var("PATHEXT");
    doze_os::which::which_in(
        binary,
        path.as_ref(),
        ext.as_deref().map(std::ffi::OsStr::new),
    )
}

/// Whether a block of the file is named `name` (Go's `Edit.Declared`).
pub(crate) fn declared(ed: &Edit, name: &str) -> bool {
    ed.blocks()
        .iter()
        .any(|(kind, labels)| match (kind.as_str(), labels.as_slice()) {
            ("module", [_, n]) | ("app" | "service" | "task", [n]) => n == name,
            _ => false,
        })
}

/// `base`, or `base2`, `base3`… when taken.
fn free_name(ed: &Edit, base: &str) -> String {
    if !declared(ed, base) {
        return base.to_owned();
    }
    (2u32..)
        .map(|i| format!("{base}{i}"))
        .find(|n| !declared(ed, n))
        .unwrap_or_else(|| base.to_owned())
}

/// What would be written has to be a config doze reads, or nothing is.
pub(crate) fn must_read(
    ctx: &Ctx<'_>,
    mode: &Mode,
    path: &Path,
    text: &[u8],
    shown: &str,
) -> Result<(), Diagnostic> {
    Loader::new(ctx, mode)
        .parse_shape(&String::from_utf8_lossy(text), path)
        .map(drop)
        .map_err(|d| {
            Diagnostic::new(
                ids::WOULD_NOT_READ,
                format!("this would leave {shown} unreadable, so nothing was written"),
            )
            .note(d.message)
            .fix("fix what is wrong in the file first: doze lint")
        })
}

pub(crate) fn write(path: &Path, bytes: &[u8], shown: &str) -> Result<(), Diagnostic> {
    std::fs::write(path, bytes).map_err(|e| {
        Diagnostic::new(
            ids::CONFIG_UNWRITABLE,
            format!("{shown} can't be written: {e}"),
        )
        .fix("check the folder's permissions and the free space")
    })
}

impl Added {
    /// Go's lines: how a service is run, the block, the wiring, and what was done.
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = String::new();
        if let [first, rest @ ..] = self.how.as_slice() {
            out.push_str(&format!("{} {first}\n", p.dim("→")));
            for l in rest {
                out.push_str(&format!("  {l}\n"));
            }
            out.push_str(&format!(
                "{} --as installed · --as container · --as download\n\n",
                p.dim("  other ways:")
            ));
        }
        let label = match self.kind.as_str() {
            "module" => format!("module {} \"{}\"", self.module, self.name),
            _ => format!("service \"{}\"", self.name),
        };
        out.push_str(&format!("{} {label}\n", p.dim("→")));
        for l in self.block.trim_end().lines() {
            out.push_str(&format!("  {l}\n"));
        }
        if let Some(w) = &self.wiring {
            out.push_str(&format!(
                "\n{}  {} = {}\n",
                p.dim("  use it from an app:"),
                w.env,
                w.reference
            ));
        }
        if self.written {
            out.push_str(&format!(
                "\n{} added to {}{}\n",
                p.ok("✓"),
                self.file,
                p.dim(" — doze lint · doze up")
            ));
        } else {
            out.push_str(&format!("{}\n", p.dim("\n(dry run: nothing written)")));
        }
        out
    }
}
