//! Function calls (hclsyntax's `FunctionCallExpr`): look the name up, expand a final `...` argument,
//! check the count, convert each argument to its parameter's type, then call with cty's rules.

use super::{Context, Evaluator, Out, suggestion};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::function::{self, Function, Param};
use crate::types::Type;
use crate::value::Value;
use crate::value::quote::go_quote;
use hcl_edit::expr::{Expression, FuncCall};

/// An argument: its value (once evaluated) and where it came from.
struct Arg<'a> {
    expr: Option<&'a Expression>,
    value: Option<Value>,
    at: Range,
}

impl Evaluator<'_> {
    pub(super) fn func_call(&self, f: &FuncCall, ctx: &Context<'_>) -> Out {
        let whole = self.range(f);
        let name_at = match f.name.namespace.first() {
            Some(first) => Range::between(self.range(first), self.range(&f.name.name)),
            None => self.range(&f.name.name),
        };
        let name = f
            .name
            .namespace
            .iter()
            .map(|n| format!("{}::", n.as_str()))
            .collect::<String>()
            + f.name.name.as_str();
        let func = match lookup(ctx, &name) {
            Ok(func) => func,
            Err(diag) => return (Value::DYNAMIC, diag(name_at, whole).into()),
        };
        let mut diags = Diagnostics::default();
        let exprs: Vec<&Expression> = f.args.iter().collect();
        let mut args: Vec<Arg<'_>> = exprs
            .iter()
            .map(|e| Arg {
                expr: Some(e),
                value: None,
                at: self.range(*e),
            })
            .collect();
        if f.args.expand_final() {
            match self.expand(&mut args, whole, ctx) {
                Ok(d) => diags.extend(d),
                Err(out) => return out,
            }
        }
        if let Err(d) = arity(&name, func, &args, whole) {
            return (Value::DYNAMIC, d.into());
        }
        let vals = self.convert_args(func, &mut args, whole, ctx, &mut diags);
        if diags.has_errors() {
            return (Value::DYNAMIC, diags);
        }
        match function::call(func, &vals) {
            Ok(v) => (v, diags),
            Err(e) => {
                diags.push(call_error(&name, func, &e, &args, whole));
                (Value::DYNAMIC, diags)
            }
        }
    }

    /// Replaces the final argument with the elements of its tuple, list or set value.
    fn expand<'a>(
        &self,
        args: &mut Vec<Arg<'a>>,
        whole: Range,
        ctx: &Context<'_>,
    ) -> Result<Diagnostics, Out> {
        let Some(last) = args.pop() else {
            return Ok(Diagnostics::default());
        };
        let Some(expr) = last.expr else {
            return Ok(Diagnostics::default());
        };
        let (v, diags) = self.eval(expr, ctx);
        if diags.has_errors() {
            return Err((Value::DYNAMIC, diags));
        }
        let bad = |detail: &str, mut diags: Diagnostics| {
            diags.push(
                Diagnostic::error("Invalid expanding argument value", detail, last.at)
                    .with_context(whole),
            );
            Err((Value::DYNAMIC, diags))
        };
        let ty = v.ty();
        let sequence = matches!(ty, Type::Tuple(_) | Type::List(_) | Type::Set(_));
        if ty != Type::Dynamic && !sequence {
            return bad(
                "The expanding argument (indicated by ...) must be of a tuple, list, or set type.",
                diags,
            );
        }
        if v.is_null() {
            return bad(
                "The expanding argument (indicated by ...) must not be null.",
                diags,
            );
        }
        if ty == Type::Dynamic || !v.is_known() {
            return Err((Value::DYNAMIC, diags));
        }
        for (_, e) in v.elements() {
            args.push(Arg {
                expr: None,
                value: Some(e),
                at: last.at,
            });
        }
        Ok(diags)
    }

    fn convert_args(
        &self,
        func: &dyn Function,
        args: &mut [Arg<'_>],
        whole: Range,
        ctx: &Context<'_>,
        diags: &mut Diagnostics,
    ) -> Vec<Value> {
        let params = func.params();
        let mut vals = Vec::with_capacity(args.len());
        for (i, arg) in args.iter_mut().enumerate() {
            let Some(param) = params.get(i).or_else(|| func.var_param()) else {
                break;
            };
            let given = match (arg.value.take(), arg.expr) {
                (Some(v), _) => v,
                (None, Some(e)) => {
                    let (v, d) = self.eval(e, ctx);
                    diags.extend(d);
                    v
                }
                (None, None) => Value::DYNAMIC,
            };
            let v = match convert(&given, &param.ty) {
                Ok(v) => v,
                Err(e) => {
                    diags.push(invalid_arg(param, &e.0, start_of(arg.at), whole));
                    given
                }
            };
            arg.value = Some(v.clone());
            vals.push(v);
        }
        vals
    }
}

type LookupError = Box<dyn Fn(Range, Range) -> Diagnostic>;

fn lookup<'f>(ctx: &'f Context<'_>, name: &str) -> Result<&'f dyn Function, LookupError> {
    let mut any = false;
    let mut names = Vec::new();
    for c in ctx.scopes() {
        if let Some(fs) = c.functions {
            any = true;
            if let Some(f) = fs.get(name) {
                return Ok(f.as_ref());
            }
            names.extend(fs.keys().cloned());
        }
    }
    if !any {
        return Err(Box::new(|at, _| {
            Diagnostic::error(
                "Function calls not allowed",
                "Functions may not be called here.",
                at,
            )
        }));
    }
    let hint = suggestion(name, &names)
        .map(|s| format!(" Did you mean {}?", go_quote(&s)))
        .unwrap_or_default();
    let detail = format!("There is no function named {}.{hint}", go_quote(name));
    Err(Box::new(move |at, whole| {
        Diagnostic::error("Call to unknown function", detail.clone(), at).with_context(whole)
    }))
}

fn invalid_arg(param: &Param, msg: &str, at: Range, whole: Range) -> Diagnostic {
    let detail = format!(
        "Invalid value for {} parameter: {msg}.",
        go_quote(param.name)
    );
    Diagnostic::error("Invalid function argument", detail, at).with_context(whole)
}

fn call_error(
    name: &str,
    func: &dyn Function,
    e: &function::FuncError,
    args: &[Arg<'_>],
    whole: Range,
) -> Diagnostic {
    let failed = || {
        let detail = format!("Call to function {} failed: {}.", go_quote(name), e.msg);
        Diagnostic::error("Error in function call", detail, start_of(whole)).with_context(whole)
    };
    let Some(i) = e.arg else { return failed() };
    let param = func.params().get(i).or_else(|| func.var_param());
    match (param, args.get(i)) {
        (Some(p), Some(a)) => invalid_arg(p, &e.msg, start_of(a.at), whole),
        (Some(p), None) => {
            let detail = format!(
                "Invalid value for {} parameter: {}.",
                go_quote(p.name),
                e.msg
            );
            Diagnostic::error("Invalid function argument", detail, whole)
        }
        (None, _) => failed(),
    }
}

/// Go's `StartRange` for an expression: for diagnostics here, the expression's own range (Go
/// narrows it for a few kinds; the start position is the same).
fn start_of(r: Range) -> Range {
    r
}

/// The `)` that ends a call: the last byte of its span.
fn close_paren(whole: Range) -> Range {
    let mut start = whole.end;
    start.byte = start.byte.saturating_sub(1);
    start.column = start.column.saturating_sub(1);
    Range {
        start,
        end: whole.end,
    }
}

/// Too few or too many arguments for the function.
fn arity(
    name: &str,
    func: &dyn Function,
    args: &[Arg<'_>],
    whole: Range,
) -> Result<(), Diagnostic> {
    let params = func.params();
    if let Some(missing) = params.get(args.len()) {
        let qual = if func.var_param().is_some() {
            " at least"
        } else {
            ""
        };
        let detail = format!(
            "Function {} expects{qual} {} argument(s). Missing value for {}.",
            go_quote(name),
            params.len(),
            go_quote(missing.name)
        );
        return Err(
            Diagnostic::error("Not enough function arguments", detail, close_paren(whole))
                .with_context(whole),
        );
    }
    if func.var_param().is_none() && args.len() > params.len() {
        let at = args.get(params.len()).map_or(whole, |a| start_of(a.at));
        let detail = format!(
            "Function {} expects only {} argument(s).",
            go_quote(name),
            params.len()
        );
        return Err(
            Diagnostic::error("Too many function arguments", detail, at).with_context(whole)
        );
    }
    Ok(())
}
