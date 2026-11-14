//! `module` and `app` blocks declared: the meta-arguments, the stamps, and the arguments every
//! block has (Go's `instance.go`).

use crate::body::{Block, Body, attrs_schema};
use crate::error::{At, Error, Result, diag_error, nearest, pos_err};
use crate::eval::{decode_bool, decode_string, eval};
use crate::foreach::{Stamp, parse_depends_on, stamps};
use crate::gohcl::{Decoded, Spec, Ty, decode_body, opt};
use crate::kinds::{Head, head_of};
use crate::load::{Loader, Pending, Probes};
use crate::model::{Config, Instance, Kind, Sleep};
use crate::modules::ModuleInfo;
use crate::runspec::parse_sleep;
use doze_hcl::Value;
use doze_hcl::value::quote::go_quote;
use hcl_edit::Span;
use std::collections::BTreeMap;

static COMMON: Spec = Spec {
    attrs: &[
        opt("version", Ty::Str),
        opt("listen", Ty::Str),
        opt("port", Ty::Int),
        opt("domains", Ty::StrList),
        opt("cert", Ty::Str),
        opt("key", Ty::Str),
    ],
    blocks: &[],
    remain: true,
};

/// What a module block's own `sleep` and `routes` say.
#[derive(Debug, Default)]
struct Core {
    sleep: Sleep,
    sleep_at: Option<At>,
    no_routes: bool,
}

impl<'a> Loader<'a> {
    /// Go's `expandInstanceBlock`: one pending instance per stamp.
    pub(crate) fn expand_instance(
        &mut self,
        cfg: &mut Config,
        block: &Block<'a>,
    ) -> Result<Vec<Pending<'a>>> {
        let files = self.files;
        let head = head_of(files, block)?;
        let meta_schema = attrs_schema(&["count", "for_each", "depends_on", "enabled", "after"]);
        let (meta, rest, ds) = block.body.partial_content(&meta_schema);
        if ds.has_errors() {
            return Err(diag_error(files, ds));
        }
        let (count, for_each) = (meta.attr("count"), meta.attr("for_each"));
        if count.is_some() && for_each.is_some() {
            return Err(pos_err(
                files,
                &block.def,
                format!(
                    "{}: set either count or for_each, not both",
                    head.title(&head.label)
                ),
                "",
            ));
        }
        let enabled = self.enabled(meta.attr("enabled"), &head.title(&head.label))?;
        let explicit = match meta.attr("depends_on") {
            Some(a) => self.in_ctx(None, Probes::No, |ctx| parse_depends_on(files, a, ctx))?,
            None => BTreeMap::new(),
        };
        let stamps = self.in_ctx(None, Probes::No, |ctx| stamps(files, count, for_each, ctx))?;
        let mut out = Vec::with_capacity(stamps.len());
        for s in stamps {
            let mut p = self.build_pending(cfg, block, &head, &rest, &s)?;
            if let Some(i) = cfg.instances.get_mut(p.idx) {
                i.enabled = enabled;
            }
            p.explicit = explicit.clone();
            p.depends_on = meta.attr("depends_on").cloned();
            p.after = meta.attr("after").cloned();
            out.push(p);
        }
        Ok(out)
    }

    /// `enabled = <bool>`, true when unset.
    pub(crate) fn enabled(
        &self,
        attr: Option<&doze_hcl::decode::Attr<'_>>,
        what: &str,
    ) -> Result<bool> {
        let Some(a) = attr else { return Ok(true) };
        let (v, ds) = self.in_ctx(None, Probes::No, |ctx| eval(a, ctx));
        if ds.has_errors() {
            return Err(diag_error(self.files, ds));
        }
        match v {
            Value::Bool(b) => Ok(b),
            _ => Err(pos_err(
                self.files,
                &At::new(a.file, a.range),
                format!("{what}: enabled must be a boolean"),
                "",
            )),
        }
    }

    /// The name, refused if another block has it (names are unique across kinds).
    pub(crate) fn claim_name(&mut self, what: &str, name: &str, def: &At) -> Result<()> {
        if let Some(first) = self.decl_ranges.get(name) {
            return Err(pos_err(
                self.files,
                def,
                format!("{what}: the name {} is already taken", go_quote(name)),
                format!(
                    "first declared at {first} — names are unique across a stack, whatever the kind"
                ),
            ));
        }
        self.decl_ranges.insert(name.to_owned(), def.clone());
        Ok(())
    }

    fn build_pending(
        &mut self,
        cfg: &mut Config,
        block: &Block<'a>,
        head: &Head,
        rest: &Body<'a>,
        s: &Stamp,
    ) -> Result<Pending<'a>> {
        let files = self.files;
        let name = s.name(&head.label);
        let what = head.title(&name);
        self.claim_name(&what, &name, &block.def)?;
        let (c, _, _) = rest.partial_content(&attrs_schema(&["wakes_on"]));
        if let Some(a) = c.attr("wakes_on") {
            return Err(pos_err(
                files,
                &At::new(a.file, a.range),
                format!("{what}: `wakes_on` isn't in this release"),
                "delete it; until it ships, a worker with no port stays on",
            ));
        }
        let mut rest = rest.clone();
        let mut core = Core::default();
        if head.kind == Kind::Module {
            (core, rest) = self.module_core(&rest, s, &what)?;
        }
        self.check_version_literal(&rest, &what)?;
        let (c, ds) = self.in_ctx(s.vars.as_ref(), Probes::No, |ctx| {
            decode_body(&rest, ctx, &COMMON)
        });
        if ds.has_errors() {
            return Err(diag_error(files, ds));
        }
        let module = self.module_for(block, head, &c)?;
        let inst = self.instance(block, head, &c, module.as_ref(), name)?;
        let idx = cfg.instances.len();
        cfg.instances.push(Instance {
            index: idx,
            sleep: core.sleep,
            no_routes: core.no_routes,
            ..inst
        });
        Ok(Pending {
            idx,
            module,
            body: rest,
            remain: c.remain.clone().unwrap_or_else(|| block.body.clone()),
            stamp: s.vars.clone(),
            explicit: BTreeMap::new(),
            depends_on: None,
            after: None,
            def: block.def.clone(),
            sleep_at: core.sleep_at,
            base_dir: self.base_dir(&block.def),
        })
    }

    fn module_core(&self, rest: &Body<'a>, s: &Stamp, what: &str) -> Result<(Core, Body<'a>)> {
        let files = self.files;
        let (c, rest, ds) = rest.partial_content(&attrs_schema(&["sleep", "routes"]));
        if ds.has_errors() {
            return Err(diag_error(files, ds));
        }
        let mut core = Core::default();
        if let Some(a) = c.attr("sleep") {
            let raw = self
                .in_ctx(s.vars.as_ref(), Probes::No, |ctx| decode_string(a, ctx))
                .map_err(|ds| diag_error(files, ds))?;
            let at = At::new(a.file, a.range);
            core.sleep =
                parse_sleep(&raw).map_err(|e| pos_err(files, &at, format!("{what}: {e}"), ""))?;
            core.sleep_at = Some(at);
        }
        if let Some(a) = c.attr("routes") {
            let on = self
                .in_ctx(s.vars.as_ref(), Probes::No, |ctx| decode_bool(a, ctx))
                .map_err(|_| {
                    pos_err(
                        files,
                        &At::new(a.file, a.range),
                        format!("{what}: `routes` is true or false"),
                        "routes = false serves the module's HTTP listeners on their ports only",
                    )
                })?;
            core.no_routes = !on;
        }
        Ok((core, rest))
    }

    /// `version = 10.10` reads as 10.1: a number with a fraction is refused.
    fn check_version_literal(&self, rest: &Body<'a>, what: &str) -> Result<()> {
        let (c, _, _) = rest.partial_content(&attrs_schema(&["version"]));
        let Some(a) = c.attr("version") else {
            return Ok(());
        };
        let hcl_edit::expr::Expression::Number(n) = a.expr else {
            return Ok(());
        };
        let lit = n.value();
        if lit.is_u64() || lit.is_i64() {
            return Ok(());
        }
        let text = a
            .expr
            .span()
            .and_then(|sp| a.src.text().get(sp))
            .unwrap_or("")
            .trim();
        if doze_hcl::Number::parse(text).is_some_and(|x| x.is_integer()) {
            return Ok(());
        }
        Err(pos_err(
            self.files,
            &At::new(a.file, a.range),
            format!(
                "{what}: version {text} is a number, and a number drops trailing zeros (10.10 reads as 10.1)"
            ),
            format!("write version = {}, quoted", go_quote(text)),
        ))
    }

    /// The block's module: looked up (an app's engine is built in); none for a module in the shape pass.
    fn module_for(
        &self,
        block: &Block<'a>,
        head: &Head,
        c: &Decoded<'a>,
    ) -> Result<Option<ModuleInfo>> {
        if head.kind == Kind::App {
            return Ok(Some(ModuleInfo {
                versionless: true,
                ..ModuleInfo::default()
            }));
        }
        if self.shape {
            return Ok(None);
        }
        let mods = self.opts.modules;
        if let Some(m) = mods.lookup(&head.ty, &c.str("version")) {
            return Ok(Some(m));
        }
        if let Some(e) = mods.lookup_error(&head.ty) {
            return Err(pos_err(self.files, &block.def, e, ""));
        }
        let names: Vec<String> = mods
            .names()
            .into_iter()
            .filter(|n| n != crate::model::COMMAND_ENGINE)
            .collect();
        let mut summary = format!("no module called {}", go_quote(&head.ty));
        if let Some(s) = nearest(&head.ty, &names) {
            summary.push_str(&format!(". Did you mean {}?", go_quote(&s)));
        }
        Err(pos_err(self.files, &block.def, summary, ""))
    }

    fn instance(
        &self,
        block: &Block<'a>,
        head: &Head,
        c: &Decoded<'a>,
        module: Option<&ModuleInfo>,
        name: String,
    ) -> Result<Instance> {
        let files = self.files;
        let what = head.title(&name);
        let (mut version, listen) = (c.str("version"), c.str("listen"));
        if head.kind == Kind::App && (!version.is_empty() || !listen.is_empty()) {
            let arg = if version.is_empty() {
                "listen"
            } else {
                "version"
            };
            return Err(pos_err(
                files,
                &block.def,
                format!("{what}: `{arg}` is not an app argument"),
                "",
            ));
        }
        if version.is_empty() {
            if module.is_some_and(|m| !m.versionless) {
                return Err(pos_err(
                    files,
                    &block.def,
                    format!("{what}: missing required \"version\""),
                    "add e.g. version = 16 (a major) or version = \"16.14\" (exact)",
                ));
            }
            version = "builtin".into();
        }
        let mut port = c.int("port");
        if port != 0 && !(1..=65535).contains(&port) {
            return Err(pos_err(
                files,
                &block.def,
                format!("{what}: port {port} is out of range"),
                "a port must be between 1 and 65535",
            ));
        }
        if port == 0 && listen.is_empty() && head.kind == Kind::Module {
            let info = module
                .cloned()
                .or_else(|| self.opts.modules.cached(&head.ty));
            port = i64::from(info.map_or(0, |m| m.default_port));
        }
        let base = self.base_dir(&block.def);
        let (domains, cert, key) = (c.list("domains"), c.str("cert"), c.str("key"));
        let decl = crate::validate::DomainDecl {
            domains: &domains,
            cert: &cert,
            key: &key,
            base_dir: &base,
        };
        let d = crate::validate::check_domains(self.opts.host, head, &what, decl, self.shape)
            .map_err(|e| pos_err(files, &block.def, e, ""))?;
        Ok(Instance {
            kind: Some(head.kind),
            ty: head.ty.clone(),
            name,
            version,
            listen,
            port: u16::try_from(port).map_err(|_| Error::msg("port out of range"))?,
            domains: d.0,
            cert: d.1,
            key: d.2,
            enabled: true,
            ..Instance::default()
        })
    }
}
