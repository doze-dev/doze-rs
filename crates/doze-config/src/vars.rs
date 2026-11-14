//! `variable`, `locals` and `output` (Go's `vars.go`): input values from `--var`, `DOZE_VAR_<name>`
//! and `*.auto.doze.vars`, locals in dependency order, outputs rendered as text.

use crate::body::{Block, Body};
use crate::error::{At, Error, Result, Sources, diag_error, nearest, pos_err};
use crate::eval::{eval, expr_is_null};
use crate::gohcl::{Spec, Ty, decode_body, opt, req};
use crate::render::render_value;
use doze_hcl::Value;
use doze_hcl::convert::convert;
use doze_hcl::eval::Context;
use doze_hcl::types::Type;
use doze_hcl::value::quote::go_quote;
use std::collections::BTreeMap;

pub const ENV_PREFIX: &str = "DOZE_VAR_";
pub const AUTO_VARS_SUFFIX: &str = ".auto.doze.vars";

static VARIABLE: Spec = Spec {
    attrs: &[
        opt("type", Ty::Expr),
        opt("default", Ty::Expr),
        opt("description", Ty::Str),
        opt("sensitive", Ty::Bool),
    ],
    blocks: &[],
    remain: false,
};

static OUTPUT: Spec = Spec {
    attrs: &[
        req("value", Ty::Expr),
        opt("description", Ty::Str),
        opt("sensitive", Ty::Bool),
    ],
    blocks: &[],
    remain: false,
};

/// Where variable values come from, besides defaults.
pub struct Inputs<'a> {
    pub cli: &'a BTreeMap<String, String>,
    pub auto: BTreeMap<String, Value>,
    pub host: Option<&'a dyn crate::host::Host>,
}

impl std::fmt::Debug for Inputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs").field("cli", &self.cli).finish()
    }
}

/// Go's `resolveVariables`: every `variable` block's value.
pub fn resolve_variables(
    src: &dyn Sources,
    blocks: &[Block<'_>],
    inputs: &Inputs<'_>,
    ctx: &Context<'_>,
) -> Result<BTreeMap<String, Value>> {
    let mut vars = BTreeMap::new();
    let mut seen: BTreeMap<String, At> = BTreeMap::new();
    for b in blocks {
        let name = b.label(0).to_owned();
        if let Some(first) = seen.get(&name) {
            return Err(pos_err(
                src,
                &b.def,
                format!("variable {} is already declared", go_quote(&name)),
                format!("first declared at {first}"),
            ));
        }
        seen.insert(name.clone(), b.def.clone());
        let (v, ds) = decode_body(&b.body, ctx, &VARIABLE);
        if ds.has_errors() {
            return Err(diag_error(src, ds));
        }
        let mut want = Type::Dynamic;
        if let Some(t) = v.expr("type").filter(|t| !expr_is_null(t)) {
            let (ty, ds) = doze_hcl::typeexpr::type_constraint(t.src, t.expr);
            if ds.has_errors() {
                let ds = doze_hcl::diag::Diagnostics(
                    ds.0.into_iter().map(|d| d.in_file(t.file)).collect(),
                );
                return Err(diag_error(src, ds));
            }
            want = ty;
        }
        let one = One {
            src,
            name: &name,
            want: &want,
            at: &b.def,
        };
        vars.insert(name.clone(), one.resolve(v.expr("default"), inputs, ctx)?);
    }
    stray_cli(inputs.cli, &seen)?;
    Ok(vars)
}

fn stray_cli(cli: &BTreeMap<String, String>, seen: &BTreeMap<String, At>) -> Result<()> {
    let Some(stray) = cli.keys().find(|n| !seen.contains_key(*n)) else {
        return Ok(());
    };
    let declared: Vec<String> = seen.keys().cloned().collect();
    let mut msg = format!(
        "--var {stray}: this stack declares no variable {}",
        go_quote(stray)
    );
    if let Some(near) = nearest(stray, &declared) {
        msg.push_str(&format!("; did you mean {near}?"));
    } else if !declared.is_empty() {
        msg.push_str(&format!("; it declares {}", declared.join(", ")));
    } else {
        msg.push_str(&format!(
            "; it declares none (a variable \"{stray}\" {{ … }} block makes one)"
        ));
    }
    Err(Error::msg(msg))
}

struct One<'s> {
    src: &'s dyn Sources,
    name: &'s str,
    want: &'s Type,
    at: &'s At,
}

impl One<'_> {
    fn resolve(
        &self,
        default: Option<&doze_hcl::decode::Attr<'_>>,
        inputs: &Inputs<'_>,
        ctx: &Context<'_>,
    ) -> Result<Value> {
        if let Some(s) = inputs.cli.get(self.name) {
            return self.convert(self.parse_text(s, "--var")?);
        }
        let env_name = format!("{ENV_PREFIX}{}", self.name);
        if let Some(s) = inputs.host.and_then(|h| h.env(&env_name)) {
            return self.convert(self.parse_text(&s, &env_name)?);
        }
        if let Some(v) = inputs.auto.get(self.name) {
            return self.convert(v.clone());
        }
        if let Some(d) = default.filter(|d| !expr_is_null(d)) {
            let (v, ds) = eval(d, ctx);
            if ds.has_errors() {
                return Err(diag_error(self.src, ds));
            }
            return self.convert(v);
        }
        Err(pos_err(
            self.src,
            self.at,
            format!("no value for required variable {}", go_quote(self.name)),
            format!(
                "set it with --var {n}=…, the {ENV_PREFIX}{n} env var, a {AUTO_VARS_SUFFIX} file, or a default",
                n = self.name
            ),
        ))
    }

    fn convert(&self, v: Value) -> Result<Value> {
        if *self.want == Type::Dynamic {
            return Ok(v);
        }
        convert(&v, self.want).map_err(|e| {
            pos_err(
                self.src,
                self.at,
                format!("variable {}: invalid value", go_quote(self.name)),
                e.0,
            )
        })
    }

    /// Go's `parseStringValue`: text as the variable's type (an expression for a collection).
    fn parse_text(&self, s: &str, source: &str) -> Result<Value> {
        let q = go_quote(self.name);
        match self.want {
            Type::String | Type::Dynamic => return Ok(Value::string(s)),
            Type::Number | Type::Bool => {
                return convert(&Value::string(s), self.want).map_err(|e| {
                    pos_err(
                        self.src,
                        self.at,
                        format!("variable {q} from {source}: invalid value {}", go_quote(s)),
                        e.0,
                    )
                });
            }
            _ => {}
        }
        match doze_hcl::eval::eval_attribute(&format!("x = {s}\n"), "x", &Context::default()) {
            Ok((v, ds)) if !ds.has_errors() => Ok(v),
            Ok((_, ds)) => Err(pos_err(
                self.src,
                self.at,
                format!("variable {q} from {source}: invalid value"),
                ds.to_string(),
            )),
            Err(d) => Err(pos_err(
                self.src,
                self.at,
                format!("variable {q} from {source}: not a valid expression"),
                d.to_string(),
            )),
        }
    }
}

/// Go's `evaluateLocals`: every local, each after the locals it reads.
pub fn evaluate_locals(
    src: &dyn Sources,
    blocks: &[Block<'_>],
    vars: &BTreeMap<String, Value>,
    funcs: &doze_hcl::function::Functions,
) -> Result<BTreeMap<String, Value>> {
    let mut exprs = BTreeMap::new();
    for b in blocks {
        let (attrs, ds) = b.body.just_attributes();
        if ds.has_errors() {
            return Err(diag_error(src, ds));
        }
        for (n, a) in attrs {
            if let Some(first) = exprs.get(&n) {
                let first: &doze_hcl::decode::Attr<'_> = first;
                return Err(pos_err(
                    src,
                    &At::new(a.file, a.name_range),
                    format!("local {} is already declared", go_quote(&n)),
                    format!(
                        "first declared at {}",
                        At::new(first.file, first.name_range)
                    ),
                ));
            }
            exprs.insert(n.clone(), a);
        }
    }
    let mut l = Locals {
        src,
        exprs,
        vars,
        funcs,
        done: BTreeMap::new(),
        visiting: Vec::new(),
    };
    let names: Vec<String> = l.exprs.keys().cloned().collect();
    for n in &names {
        l.resolve(n, &mut Vec::new())?;
    }
    Ok(l.done)
}

struct Locals<'s, 'a> {
    src: &'s dyn Sources,
    exprs: BTreeMap<String, doze_hcl::decode::Attr<'a>>,
    vars: &'s BTreeMap<String, Value>,
    funcs: &'s doze_hcl::function::Functions,
    done: BTreeMap<String, Value>,
    visiting: Vec<String>,
}

impl Locals<'_, '_> {
    fn resolve(&mut self, n: &str, stack: &mut Vec<String>) -> Result<()> {
        if self.done.contains_key(n) {
            return Ok(());
        }
        let Some(attr) = self.exprs.get(n).cloned() else {
            return Ok(());
        };
        if self.visiting.iter().any(|v| v == n) {
            let mut cycle = stack.clone();
            cycle.push(n.to_owned());
            return Err(pos_err(
                self.src,
                &At::new(attr.file, attr.name_range),
                "cycle between locals",
                cycle.join(" → "),
            ));
        }
        self.visiting.push(n.to_owned());
        for t in crate::refs::variables(attr.expr, attr.src) {
            let Some(dep) = t
                .name()
                .filter(|d| t.root == "local" && *d != n)
                .map(str::to_owned)
            else {
                continue;
            };
            if self.exprs.contains_key(&dep) {
                stack.push(n.to_owned());
                let r = self.resolve(&dep, stack);
                stack.pop();
                r?;
            }
        }
        let mut vars = self.vars.clone();
        vars.insert("local".into(), Value::Object(self.done.clone()));
        let ctx = Context::new(vars, Some(self.funcs));
        let (v, ds) = eval(&attr, &ctx);
        if ds.has_errors() {
            return Err(diag_error(self.src, ds));
        }
        self.done.insert(n.to_owned(), v);
        Ok(())
    }
}

/// Go's `evaluateOutputs`: each output's value as text.
pub fn evaluate_outputs(
    src: &dyn Sources,
    blocks: &[Block<'_>],
    ctx: &Context<'_>,
) -> Result<(BTreeMap<String, crate::model::Output>, Vec<String>)> {
    let mut out = BTreeMap::new();
    let mut order = Vec::new();
    let mut seen: BTreeMap<String, At> = BTreeMap::new();
    for b in blocks {
        let name = b.label(0).to_owned();
        if let Some(first) = seen.get(&name) {
            return Err(pos_err(
                src,
                &b.def,
                format!("output {} is already declared", go_quote(&name)),
                format!("first declared at {first}"),
            ));
        }
        seen.insert(name.clone(), b.def.clone());
        let (o, ds) = decode_body(&b.body, ctx, &OUTPUT);
        if ds.has_errors() {
            return Err(diag_error(src, ds));
        }
        let Some(value) = o.expr("value") else {
            continue;
        };
        let (v, ds) = eval(value, ctx);
        if ds.has_errors() {
            return Err(diag_error(src, ds));
        }
        let o = crate::model::Output {
            name: name.clone(),
            value: render_value(&v),
            description: o.str("description"),
            sensitive: o.bool("sensitive"),
        };
        out.insert(name.clone(), o);
        order.push(name);
    }
    Ok((out, order))
}

/// Go's `loadAutoVars`: every `*.auto.doze.vars` in the config's folder, in name order.
pub fn load_auto_vars(
    host: &dyn crate::host::Host,
    dir: &str,
    funcs: &doze_hcl::function::Functions,
) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(host.abs(std::path::Path::new(dir))) else {
        return Ok(out);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(AUTO_VARS_SUFFIX))
        .collect();
    names.sort();
    for n in names {
        let path =
            crate::files::path_string(&crate::paths::clean(&std::path::Path::new(dir).join(&n)));
        let files = crate::files::read_all(host, std::slice::from_ref(&path), "")?;
        let Some(f) = files.files.first() else {
            continue;
        };
        let body = Body::new(doze_hcl::decode::BodyRef::file(&f.body, &f.src, &f.name));
        let (attrs, ds) = body.just_attributes();
        if ds.has_errors() {
            return Err(diag_error(&files, ds));
        }
        let mut ctx = Context::default();
        ctx.functions = Some(funcs);
        for (k, a) in attrs {
            let (v, ds) = eval(&a, &ctx);
            if ds.has_errors() {
                return Err(diag_error(&files, ds));
            }
            out.insert(k, v);
        }
    }
    Ok(out)
}
