//! The load: files to a validated [`Config`] (Go's `load`, `loadFiles` and `buildConfig`).

use crate::body::{Block, Body};
use crate::error::{At, Error, Result, diag_error, pos_err};
use crate::eval::decode_string;
use crate::files::{Files, path_string};
use crate::host::Host;
use crate::model::{Condition, Config, DEFAULT_IDLE, DEFAULT_LISTEN, Kind};
use crate::modules::{ModuleInfo, Modules};
use crate::{defaults, kinds, overrides, paths, validate, vars};
use doze_hcl::Value;
use doze_hcl::decode::{Attr, BodyRef, Schema};
use doze_hcl::eval::Context;
use doze_hcl::function::Functions;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Where a context sees the probe functions (`tcp()`, `http(…)`…), as Go nests its scopes: a
/// function name is suggested from the innermost scope's functions only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probes {
    /// None: the stack's own context, or a `count`/`for_each` stamp of it.
    No,
    /// Outside the stamp (a service's or task's body: Go's `decodeRunnables` scope).
    Outer,
    /// Inside the stamp (an app's body: the command engine's `DecodeConfig` scope).
    Inner,
}

/// What a load is given besides the path.
pub struct Options<'a> {
    pub host: &'a dyn Host,
    pub modules: &'a dyn Modules,
    /// `--var name=value`.
    pub vars: BTreeMap<String, String>,
    /// This doze's version, for `doze = ">= x"` ("dev" never needs newer).
    pub version: String,
}

impl std::fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("vars", &self.vars)
            .field("version", &self.version)
            .finish()
    }
}

/// A module or app block, declared and waiting for its body to be decoded.
#[derive(Debug)]
pub(crate) struct Pending<'a> {
    pub idx: usize,
    /// The module (or, for an app, a stand-in); `None` for a module in the shape pass.
    pub module: Option<ModuleInfo>,
    /// The body without the meta-arguments (references are read from it).
    pub body: Body<'a>,
    /// The body without the arguments every block has: what the module or app decodes.
    pub remain: Body<'a>,
    pub stamp: Option<BTreeMap<String, Value>>,
    pub explicit: BTreeMap<String, Condition>,
    pub depends_on: Option<Attr<'a>>,
    pub after: Option<Attr<'a>>,
    pub def: At,
    pub sleep_at: Option<At>,
    pub base_dir: String,
}

/// A service or task block, declared.
#[derive(Debug)]
pub(crate) struct Runnable<'a> {
    pub name: String,
    pub kind: Kind,
    pub def: At,
    pub body: Body<'a>,
    pub stamp: Option<BTreeMap<String, Value>>,
    pub enabled: bool,
    pub waits: BTreeMap<String, Condition>,
}

/// Every declared name and its kind, and the declared module types (Go's `declared`).
#[derive(Debug, Default)]
pub(crate) struct Declared {
    pub kind: BTreeMap<String, Kind>,
    pub module_types: BTreeSet<String>,
}

impl Declared {
    pub(crate) fn names(&self, kind: Kind) -> Vec<String> {
        self.kind
            .iter()
            .filter(|(_, k)| **k == kind)
            .map(|(n, _)| n.clone())
            .collect()
    }
}

pub(crate) struct Loader<'a> {
    pub files: &'a Files,
    pub opts: &'a Options<'a>,
    pub shape: bool,
    pub funcs: Functions,
    pub probes: Functions,
    pub vars: BTreeMap<String, Value>,
    pub decl_ranges: BTreeMap<String, At>,
    pub runnables: Vec<Runnable<'a>>,
}

impl std::fmt::Debug for Loader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loader")
            .field("shape", &self.shape)
            .finish()
    }
}

impl<'a> Loader<'a> {
    /// Runs `f` in the stack's context, inside a `count`/`for_each` stamp if any, with the probe
    /// functions if `probes`.
    pub(crate) fn in_ctx<R>(
        &self,
        stamp: Option<&BTreeMap<String, Value>>,
        probes: Probes,
        f: impl FnOnce(&Context<'_>) -> R,
    ) -> R {
        let base = Context::new(self.vars.clone(), Some(&self.funcs));
        match (probes, stamp) {
            (Probes::No, None) => f(&base),
            (Probes::No, Some(s)) => f(&base.child(s.clone())),
            (Probes::Outer, s) => {
                let p = self.probe_scope(&base);
                match s {
                    Some(s) => f(&p.child(s.clone())),
                    None => f(&p),
                }
            }
            (Probes::Inner, None) => f(&self.probe_scope(&base)),
            (Probes::Inner, Some(s)) => {
                let st = base.child(s.clone());
                f(&self.probe_scope(&st))
            }
        }
    }

    /// A scope inside `c` whose own functions are the probes.
    fn probe_scope<'c>(&'c self, c: &'c Context<'c>) -> Context<'c> {
        let mut p = c.child(BTreeMap::new());
        p.functions = Some(&self.probes);
        p
    }

    pub(crate) fn base_dir(&self, def: &At) -> String {
        let d = paths::dir_of(&def.file);
        path_string(&self.opts.host.abs(Path::new(&d)))
    }
}

fn root_schema() -> Schema {
    ["listen", "home", "data_dir", "stack", "doze"]
        .iter()
        .fold(Schema::default(), |s, a| s.attr(a, false))
        .block("defaults", &[])
        .block("tls", &[])
        .block("variable", &["name"])
        .block("locals", &[])
        .block("output", &["name"])
        .block("module", &["type", "name"])
        .block("app", &["name"])
        .block("service", &["name"])
        .block("task", &["name"])
}

/// Loads the stack at `path` (a doze.hcl, or a folder of `.hcl` files) and validates it.
pub fn load(path: &str, opts: &Options<'_>) -> Result<Config> {
    load_with(path, opts, false)
}

/// The offline pass (Go's `LoadShape`): everything that can be checked without a module.
pub fn load_shape(path: &str, opts: &Options<'_>) -> Result<Config> {
    load_with(path, opts, true)
}

fn load_with(path: &str, opts: &Options<'_>, shape: bool) -> Result<Config> {
    let (names, primary) = crate::files::gather(opts.host, path)?;
    let files = crate::files::read_all(opts.host, &names, &opts.version)?;
    let funcs = functions(opts.host);
    let dir = paths::config_dir_of(opts.host, path);
    let auto = vars::load_auto_vars(opts.host, &dir, &funcs)?;
    let (base, ov) = overrides::split(&files)?;
    let bodies = base
        .iter()
        .map(|f| BodyRef::file(&f.body, &f.src, &f.name))
        .collect();
    let input = Input {
        body: Body::new(BodyRef::merge(bodies)),
        ov: &ov,
        auto,
    };
    let mut cfg = build(&files, opts, shape, &primary, input)?;
    if !shape {
        validate::ports(&cfg, opts.modules)?;
        validate::networks(&cfg, opts.modules)?;
    }
    validate::app_domains(&cfg, opts.host)?;
    validate::domains(&cfg)?;
    cfg.path = primary;
    Ok(cfg)
}

/// Go's `Parse` (and `ParseShape` when `shape`): one source, no siblings and no auto vars.
// fuzz:parser fuzz_load
pub fn parse(src: &str, filename: &str, opts: &Options<'_>, shape: bool) -> Result<Config> {
    let mut files = Files::default();
    files.push(filename, src, &opts.version)?;
    let f = files.files.first().ok_or_else(|| Error::msg("no file"))?;
    let body = Body::new(BodyRef::file(&f.body, &f.src, &f.name));
    let ov = overrides::Overrides::new();
    let input = Input {
        body,
        ov: &ov,
        auto: BTreeMap::new(),
    };
    build(&files, opts, shape, filename, input)
}

/// The merged body to read, the overrides to lay over it, and the auto vars.
struct Input<'a, 'o> {
    body: Body<'a>,
    ov: &'o overrides::Overrides<'a>,
    auto: BTreeMap<String, Value>,
}

fn functions(host: &dyn Host) -> Functions {
    doze_hcl::funcs::doze_functions_with_env(host.env_lookup())
}

fn build<'a>(
    files: &'a Files,
    opts: &'a Options<'a>,
    shape: bool,
    primary: &str,
    input: Input<'a, '_>,
) -> Result<Config> {
    let Input { body, ov, auto } = input;
    kinds::check_doze_version(files, &opts.version)?;
    kinds::check_old_forms(files)?;
    let (mut content, ds) = body.content(&root_schema());
    if ds.has_errors() {
        return Err(diag_error(files, ds));
    }
    overrides::apply(files, ov, &mut content.blocks)?;
    let mut cfg = Config {
        listen: DEFAULT_LISTEN.into(),
        path: primary.into(),
        dir: path_string(
            &opts
                .host
                .abs(Path::new(&paths::config_dir_of(opts.host, primary))),
        ),
        ..Config::default()
    };
    cfg.defaults.idle_timeout = DEFAULT_IDLE;
    let mut l = Loader {
        files,
        opts,
        shape,
        funcs: functions(opts.host),
        probes: crate::runspec::functions(),
        vars: BTreeMap::new(),
        decl_ranges: BTreeMap::new(),
        runnables: Vec::new(),
    };
    l.build(&mut cfg, content, auto)?;
    Ok(cfg)
}

/// The top-level blocks by what they are.
#[derive(Debug, Default)]
struct Classified<'a> {
    defaults: Option<Block<'a>>,
    tls: Option<Block<'a>>,
    variables: Vec<Block<'a>>,
    locals: Vec<Block<'a>>,
    outputs: Vec<Block<'a>>,
    instances: Vec<Block<'a>>,
    runnables: Vec<Block<'a>>,
}

fn classify<'a>(files: &Files, blocks: Vec<Block<'a>>) -> Result<Classified<'a>> {
    let mut c = Classified::default();
    for b in blocks {
        match b.ty {
            "defaults" | "tls" => {
                let slot = if b.ty == "tls" {
                    &mut c.tls
                } else {
                    &mut c.defaults
                };
                if slot.is_some() {
                    return Err(pos_err(
                        files,
                        &b.def,
                        format!("duplicate {} block", b.ty),
                        format!("only one {} block is allowed across all config files", b.ty),
                    ));
                }
                *slot = Some(b);
            }
            "variable" => c.variables.push(b),
            "locals" => c.locals.push(b),
            "output" => c.outputs.push(b),
            "service" | "task" => c.runnables.push(b),
            _ => c.instances.push(b),
        }
    }
    Ok(c)
}

impl<'a> Loader<'a> {
    fn build(
        &mut self,
        cfg: &mut Config,
        content: crate::body::Content<'a>,
        auto: BTreeMap<String, Value>,
    ) -> Result<()> {
        let files = self.files;
        let c = classify(files, content.blocks)?;
        let inputs = vars::Inputs {
            cli: &self.opts.vars,
            auto,
            host: Some(self.opts.host),
        };
        let vs = self.in_ctx(None, Probes::No, |ctx| {
            vars::resolve_variables(files, &c.variables, &inputs, ctx)
        })?;
        self.vars.insert("var".into(), Value::Object(vs));
        let locals = vars::evaluate_locals(files, &c.locals, &self.vars, &self.funcs)?;
        if !c.locals.is_empty() {
            self.vars.insert("local".into(), Value::Object(locals));
        }
        self.root_attributes(cfg, &content.attributes)?;
        paths::resolve_home(cfg, self.opts.host).map_err(Error::msg)?;
        if let Some(b) = &c.defaults {
            self.in_ctx(None, Probes::No, |ctx| {
                defaults::decode_defaults(cfg, files, self.opts.host, b, ctx)
            })?;
        }
        if let Some(b) = &c.tls {
            self.in_ctx(None, Probes::No, |ctx| {
                defaults::decode_tls(cfg, files, b, ctx)
            })?;
        }
        let mut pending = Vec::new();
        for b in &c.instances {
            pending.extend(self.expand_instance(cfg, b)?);
        }
        for b in &c.runnables {
            self.declare_runnable(cfg, b)?;
        }
        let known = declared(cfg);
        self.publish_runnables(cfg);
        self.evaluate(cfg, &mut pending, &known)?;
        self.decode_runnables(cfg, &known)?;
        crate::cycles::check(cfg, files, &self.decl_ranges)?;
        for p in &pending {
            let Some(inst) = cfg.instances.get(p.idx) else {
                continue;
            };
            if p.module.is_some() && inst.kind() == Kind::Module {
                self.opts
                    .modules
                    .check_support(&inst.ty, &inst.version)
                    .map_err(|e| pos_err(files, &p.def, e, ""))?;
            }
        }
        let (outs, order) = self.in_ctx(None, Probes::No, |ctx| {
            vars::evaluate_outputs(files, &c.outputs, ctx)
        })?;
        (cfg.outputs, cfg.output_order) = (outs, order);
        cfg.warnings = crate::lint::warnings(files, cfg, &c.variables, &self.runnables);
        Ok(())
    }

    fn root_attributes(&self, cfg: &mut Config, attrs: &BTreeMap<String, Attr<'a>>) -> Result<()> {
        for (name, attr) in attrs {
            let dst = match name.as_str() {
                "home" => &mut cfg.home,
                "data_dir" => &mut cfg.data_dir,
                "stack" => &mut cfg.stack_name,
                _ => continue,
            };
            *dst = self
                .in_ctx(None, Probes::No, |ctx| decode_string(attr, ctx))
                .map_err(|ds| diag_error(self.files, ds))?;
        }
        Ok(())
    }
}

pub(crate) fn declared(cfg: &Config) -> Declared {
    let mut d = Declared::default();
    for i in &cfg.instances {
        d.kind.insert(i.name.clone(), i.kind());
        if i.kind() == Kind::Module {
            d.module_types.insert(i.ty.clone());
        }
    }
    for s in &cfg.services {
        d.kind.insert(s.name.clone(), Kind::Service);
    }
    for t in &cfg.tasks {
        d.kind.insert(t.name.clone(), Kind::Task);
    }
    d
}
