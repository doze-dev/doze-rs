//! Call-site checks for function calls: looking the name up, and the argument count.

use super::call::Arg;
use super::{Context, suggestion};
use crate::diag::{Diagnostic, Range};
use crate::function::Function;
use crate::value::quote::go_quote;

pub(super) type LookupError = Box<dyn Fn(Range, Range) -> Diagnostic>;

pub(super) fn lookup<'f>(
    ctx: &'f Context<'_>,
    name: &str,
) -> Result<&'f dyn Function, LookupError> {
    let mut any = false;
    for c in ctx.scopes() {
        if let Some(fs) = c.functions {
            any = true;
            if let Some(f) = fs.get(name) {
                return Ok(f.as_ref());
            }
        }
    }
    if !any {
        return Err(Box::new(|_, whole| {
            Diagnostic::error(
                "Function calls not allowed",
                "Functions may not be called here.",
                whole,
            )
        }));
    }
    // Go suggests from the evaluating scope's own functions only.
    let names: Vec<String> = ctx
        .functions
        .map(|fs| fs.keys().cloned().collect())
        .unwrap_or_default();
    let detail = unknown_function_detail(name, &names);
    Err(Box::new(move |at, whole| {
        Diagnostic::error("Call to unknown function", detail.clone(), at).with_context(whole)
    }))
}

/// hclsyntax's words for a name no scope has, namespaced (`ns::name`) or not.
fn unknown_function_detail(name: &str, names: &[String]) -> String {
    let Some(sep) = name.rfind("::") else {
        let hint = suggestion(name, names)
            .map(|s| format!(" Did you mean {}?", go_quote(&s)))
            .unwrap_or_default();
        return format!("There is no function named {}.{hint}", go_quote(name));
    };
    let (namespace, short) = (
        name.get(..sep + 2).unwrap_or(""),
        name.get(sep + 2..).unwrap_or(""),
    );
    let avail: Vec<String> = names
        .iter()
        .filter(|n| n.starts_with(namespace))
        .cloned()
        .collect();
    if avail.is_empty() {
        return format!(
            "There are no functions in namespace {}.",
            go_quote(namespace)
        );
    }
    let hint = suggestion(short, &avail)
        .map(|s| format!(" Did you mean {namespace}{s}?"))
        .unwrap_or_default();
    format!(
        "There is no function named {} in namespace {namespace}.{hint}",
        go_quote(short)
    )
}

/// The `)` that ends a call: the last byte of its span.
pub(super) fn close_paren(whole: Range) -> Range {
    let mut start = whole.end;
    start.byte = start.byte.saturating_sub(1);
    start.column = start.column.saturating_sub(1);
    Range {
        start,
        end: whole.end,
    }
}

/// Too few or too many arguments for the function.
pub(super) fn arity(
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
        let at = args.get(params.len()).map_or(whole, |a| a.at);
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
