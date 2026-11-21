//! doze.hcl, read through doze-config (core plan H4). The CLI reads the stack's *shape*: Go's `LoadShape`, everything
//! that can be checked without a module, so a read command never waits on the network (proposal rule 5) and a module
//! not on this machine is never blamed on the person's file (F3). What only a module can check (its arguments) is the
//! daemon's, or `lint`'s, to say.
//!
//! `-c` names a file, or a folder whose `*.hcl` files are merged (Go's `-c <dir>`); without it, the `doze.hcl` here or
//! in the nearest parent. `--var name=value` wins over `DOZE_VAR_*`, `*.auto.doze.vars` and defaults, as in Go.
//!
//! A config that does not load is drawn at print time in the proposal's style (O6): one error per diagnostic, at the
//! line and column Go gives, with the source line and a caret under the span ([`render`]).

pub(crate) mod render;

use crate::cmd::modules;
use crate::ctx::Ctx;
use crate::diag::Diagnostic;
use crate::output::Mode;
use crate::project::Project;
use doze_config::{Config, Options};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The stack as doze.hcl declares it, and where it came from.
#[derive(Debug, Clone)]
pub(crate) struct Loaded {
    pub(crate) project: Project,
    pub(crate) config: Config,
}

/// The project here (or at `-c`), read offline; exit 6 without one, exit 5 with a mistake in it.
pub(crate) fn shape(ctx: &Ctx<'_>, mode: &Mode) -> Result<Loaded, Diagnostic> {
    let project = modules::find(ctx, mode)?;
    let config = Loader::new(ctx, mode).shape(&project)?;
    Ok(Loaded { project, config })
}

/// The shape of a project already found.
pub(crate) fn shape_of(
    ctx: &Ctx<'_>,
    mode: &Mode,
    project: &Project,
) -> Result<Config, Diagnostic> {
    Loader::new(ctx, mode).shape(project)
}

/// What reading doze.hcl needs of a run (its environment, folder and `--var`s), kept so a command can read the
/// config when it turns out to need it (the daemon down) without carrying the run around.
#[derive(Clone)]
pub(crate) struct Loader {
    host: CliHost,
    vars: BTreeMap<String, String>,
}

impl std::fmt::Debug for Loader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loader")
            .field("vars", &self.vars)
            .finish_non_exhaustive()
    }
}

impl Loader {
    pub(crate) fn new(ctx: &Ctx<'_>, mode: &Mode) -> Loader {
        Loader {
            host: CliHost::new(ctx),
            vars: mode.vars.clone(),
        }
    }

    /// A loader on the process's own environment, for when there is no run (a shell asking for completions).
    pub(crate) fn process(cwd: &Path) -> Loader {
        Loader {
            host: CliHost {
                env: std::sync::Arc::new(|name: &str| {
                    if name.is_empty() || name.contains(['=', '\0']) {
                        return None;
                    }
                    std::env::var(name).ok()
                }),
                cwd: cwd.to_path_buf(),
                home: std::env::home_dir(),
                certs_skipped: std::cell::Cell::new(false),
            },
            vars: BTreeMap::new(),
        }
    }

    /// The project's shape, offline; exit 5 with what is wrong in it.
    pub(crate) fn shape(&self, project: &Project) -> Result<Config, Diagnostic> {
        let opts = self.options();
        let path = self.relative(&source(project));
        doze_config::load_shape(&path.to_string_lossy(), &opts)
            .map_err(|e| render::diagnose(&e, &self.host.cwd))
    }

    /// A config's text that is not on disk yet (what `add` or `init` would write), read as its shape at `path`
    /// (Go's `ParseShape`); exit 5 with what is wrong in it.
    pub(crate) fn parse_shape(&self, src: &str, path: &Path) -> Result<Config, Diagnostic> {
        let opts = self.options();
        let path = self.relative(path);
        doze_config::parse(src, &path.to_string_lossy(), &opts, true)
            .map_err(|e| render::diagnose(&e, &self.host.cwd))
    }

    /// The full load (Go's `Load`) of a stack with no module blocks: everything the shape pass checks, and the rest
    /// (outputs, `doze = ">= x"`, lint warnings). Also says whether a certificate went unchecked.
    pub(crate) fn full_without_modules(
        &self,
        project: &Project,
    ) -> Result<(Config, bool), Diagnostic> {
        let opts = self.options();
        self.host.certs_skipped.set(false);
        let path = self.relative(&source(project));
        let cfg = doze_config::load(&path.to_string_lossy(), &opts)
            .map_err(|e| render::diagnose(&e, &self.host.cwd))?;
        Ok((cfg, self.host.certs_skipped.get()))
    }

    /// The files the config is made of, in Go's order: the folder's `*.hcl`, or the file and its `*.doze.hcl`
    /// siblings.
    pub(crate) fn files(&self, project: &Project) -> Result<Vec<PathBuf>, Diagnostic> {
        let path = self.relative(&source(project));
        doze_config::files::gather(&self.host, &path.to_string_lossy())
            .map(|(names, _)| names.into_iter().map(|n| self.host.cwd.join(n)).collect())
            .map_err(|e| render::diagnose(&e, &self.host.cwd))
    }

    /// A path as the person would type it, relative to where they are when it is under there (Go's configPath is
    /// what was typed, so its diagnostics and warnings name `doze.hcl`, not an absolute path).
    fn relative(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(&self.host.cwd) {
            // The folder doze runs in is `.` (Go keeps `-c .` as typed).
            Ok(r) if r.as_os_str().is_empty() => PathBuf::from("."),
            Ok(r) => r.to_path_buf(),
            Err(_) => path.to_path_buf(),
        }
    }

    /// How doze-config is asked: this machine, no modules (the CLI reads offline), the `--var`s.
    fn options(&self) -> Options<'_> {
        Options {
            host: &self.host,
            modules: &doze_config::NoModules,
            vars: self.vars.clone(),
            version: crate::ctx::VERSION.to_owned(),
        }
    }

    /// Where the run is, for showing paths relative to it.
    pub(crate) fn cwd(&self) -> &Path {
        &self.host.cwd
    }
}

/// What to load: the folder when `-c` named one (its `*.hcl` files merge), else the file.
fn source(project: &Project) -> PathBuf {
    match &project.source {
        Some(dir) => dir.clone(),
        None => project.config.clone(),
    }
}

/// doze-config's view of this machine, through the run's own surroundings (so a test's environment is the one read).
#[derive(Clone)]
struct CliHost {
    env: doze_hcl::funcs::EnvLookup,
    cwd: PathBuf,
    home: Option<PathBuf>,
    /// A full load met a certificate it did not check.
    certs_skipped: std::cell::Cell<bool>,
}

impl CliHost {
    fn new(ctx: &Ctx<'_>) -> CliHost {
        // The doze home is the one this run uses, whether it came from `DOZE_HOME` or the default.
        let (outer, doze_home) = (ctx.host.env_lookup(), ctx.home.clone());
        let env: doze_hcl::funcs::EnvLookup = std::sync::Arc::new(move |name: &str| {
            outer(name).or_else(|| (name == "DOZE_HOME").then(|| doze_home.display().to_string()))
        });
        let home = env("HOME").map(PathBuf::from).or_else(std::env::home_dir);
        CliHost {
            env,
            cwd: ctx.cwd.clone(),
            home,
            certs_skipped: std::cell::Cell::new(false),
        }
    }
}

impl doze_config::Host for CliHost {
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        self.env.clone()
    }
    fn cwd(&self) -> PathBuf {
        self.cwd.clone()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }
    fn valid_domain(&self, host: &str) -> Result<(), String> {
        doze_names::domains::valid_domain(host)
    }
    fn route_domain(&self, host: &str) -> String {
        doze_names::domains::route_domain(host)
    }
    fn check_certificate(&self, _cert: &Path, _key: &Path, _domain: &str) -> Result<(), String> {
        // The CLI does not read certificates (the daemon does when it serves one): the load goes on, and the caller
        // is told one was left unchecked, so it never claims it was (rule 3).
        self.certs_skipped.set(true);
        Ok(())
    }
}
