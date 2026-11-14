//! Evaluating expressions with hclsyntax's semantics, on hcl-edit's syntax tree.
//!
//! Every function returns the value and the diagnostics of its own evaluation, as Go's
//! `Expression.Value` does, because Go's rules often depend on whether one part had errors.

mod arith;
mod call;
mod call_checks;
mod collections;
mod conditional;
mod for_cond;
mod for_expr;
mod ops;
mod splat;
mod template;
mod tokens;
mod traversal;

use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::function::Functions;
use crate::number::Number;
use crate::syntax::Source;
use crate::value::Value;
use hcl_edit::Span;
use hcl_edit::expr::Expression;
use std::collections::BTreeMap;

/// What an expression can see: variables and functions, in nested scopes (Go's `hcl.EvalContext`).
/// `None` for either is Go's nil map: "not allowed here", which differs from "allowed, but none".
#[derive(Default)]
pub struct Context<'p> {
    pub variables: Option<BTreeMap<String, Value>>,
    pub functions: Option<&'p Functions>,
    parent: Option<&'p Context<'p>>,
}

impl<'p> Context<'p> {
    pub fn new(
        variables: BTreeMap<String, Value>,
        functions: Option<&'p Functions>,
    ) -> Context<'p> {
        Context {
            variables: Some(variables),
            functions,
            parent: None,
        }
    }

    /// A scope inside this one, with its own variables.
    pub fn child(&'p self, variables: BTreeMap<String, Value>) -> Context<'p> {
        Context {
            variables: Some(variables),
            functions: None,
            parent: Some(self),
        }
    }

    fn scopes(&self) -> impl Iterator<Item = &Context<'p>> {
        std::iter::successors(Some(self), |c| c.parent)
    }

    /// A variable, looked up from the innermost scope out. `Err(true)` when no scope allows
    /// variables at all.
    fn lookup(&self, name: &str) -> Result<&Value, bool> {
        let mut any = false;
        for c in self.scopes() {
            if let Some(vars) = &c.variables {
                any = true;
                if let Some(v) = vars.get(name) {
                    return Ok(v);
                }
            }
        }
        Err(!any)
    }

    fn variable_names(&self) -> Vec<String> {
        self.scopes()
            .filter_map(|c| c.variables.as_ref())
            .flat_map(|v| v.keys().cloned())
            .collect()
    }
}

pub(crate) type Out = (Value, Diagnostics);

fn ok(v: Value) -> Out {
    (v, Diagnostics::default())
}

/// Evaluates expressions from one source text (for positions).
#[derive(Debug)]
pub struct Evaluator<'s> {
    src: &'s Source,
}

impl<'s> Evaluator<'s> {
    pub fn new(src: &'s Source) -> Evaluator<'s> {
        Evaluator { src }
    }

    pub(crate) fn range<T: Span + ?Sized>(&self, node: &T) -> Range {
        node.span().map(|s| self.src.range(s)).unwrap_or_default()
    }

    pub(crate) fn span_range(&self, span: Option<std::ops::Range<usize>>) -> Range {
        span.map(|s| self.src.range(s)).unwrap_or_default()
    }

    pub fn eval(&self, e: &Expression, ctx: &Context<'_>) -> Out {
        match e {
            Expression::Null(_) => ok(Value::Null(crate::Type::Dynamic)),
            Expression::Bool(b) => ok(Value::Bool(*b.value())),
            Expression::Number(n) => self.number(n),
            Expression::String(s) => ok(Value::string(s.value())),
            Expression::Array(a) => self.array(a, ctx),
            Expression::Object(o) => self.object(o, ctx),
            Expression::StringTemplate(t) => self.string_template(t, ctx),
            Expression::HeredocTemplate(h) => self.heredoc(h, ctx),
            Expression::Parenthesis(p) => self.eval(p.inner(), ctx),
            Expression::Variable(v) => self.variable(v.as_str(), self.range(v), ctx),
            Expression::Conditional(c) => self.conditional(c, ctx),
            Expression::FuncCall(f) => self.func_call(f, ctx),
            Expression::Traversal(t) => self.traversal(t, ctx),
            Expression::UnaryOp(u) => self.unary(u, ctx),
            Expression::BinaryOp(b) => self.binary(b, ctx),
            Expression::ForExpr(f) => self.for_expr(f, ctx),
        }
    }

    /// A number literal, read from its text at 512 bits (hcl-edit's own value is an f64).
    fn number(&self, f: &hcl_edit::repr::Formatted<hcl_edit::Number>) -> Out {
        let text = f
            .as_repr()
            .map_or_else(|| f.value().to_string(), |r| r.to_string());
        let neg = text.starts_with('-');
        let digits = text
            .find(|c: char| c.is_ascii_digit())
            .and_then(|i| text.get(i..))
            .unwrap_or("");
        match Number::parse(digits) {
            Some(v) => ok(Value::Number(if neg { v.neg() } else { v })),
            None => {
                let d = Diagnostic::error(
                    "Invalid number literal",
                    "Failed to recognize the value of this number literal.",
                    self.range(f),
                );
                (Value::DYNAMIC, d.into())
            }
        }
    }

    fn variable(&self, name: &str, at: Range, ctx: &Context<'_>) -> Out {
        match ctx.lookup(name) {
            Ok(v) => ok(v.clone()),
            Err(true) => (
                Value::DYNAMIC,
                Diagnostic::error(
                    "Variables not allowed",
                    "Variables may not be used here.",
                    at,
                )
                .into(),
            ),
            Err(false) => {
                let hint = suggestion(name, &ctx.variable_names())
                    .map(|s| format!(" Did you mean {}?", crate::value::quote::go_quote(&s)));
                let detail = format!(
                    "There is no variable named {}.{}",
                    crate::value::quote::go_quote(name),
                    hint.unwrap_or_default()
                );
                (
                    Value::DYNAMIC,
                    Diagnostic::error("Unknown variable", detail, at).into(),
                )
            }
        }
    }
}

/// Go's `nameSuggestion`: the first candidate within edit distance 2.
pub(crate) fn suggestion(given: &str, candidates: &[String]) -> Option<String> {
    let mut sorted: Vec<&String> = candidates.iter().collect();
    sorted.sort();
    sorted
        .into_iter()
        .find(|c| levenshtein(given, c) < 3)
        .cloned()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = Vec::with_capacity(b.len() + 1);
        cur.push(i + 1);
        for (j, cb) in b.iter().enumerate() {
            let sub = prev.get(j).copied().unwrap_or(0) + usize::from(ca != *cb);
            let del = prev.get(j + 1).copied().unwrap_or(0) + 1;
            let ins = cur.last().copied().unwrap_or(0) + 1;
            cur.push(sub.min(del).min(ins));
        }
        prev = cur;
    }
    prev.last().copied().unwrap_or(0)
}

/// Evaluates the single expression in `x = <expr>` of `text`, for tests and the oracle.
pub fn eval_attribute(text: &str, name: &str, ctx: &Context<'_>) -> Result<Out, Box<Diagnostic>> {
    let src = Source::new(text);
    let body =
        crate::syntax::parse_body(&src).map_err(|ds| {
            let p = src.pos(0);
            Box::new(ds.0.into_iter().next().unwrap_or_else(|| {
                Diagnostic::error("Syntax error", "", Range { start: p, end: p })
            }))
        })?;
    let Some(attr) = body.get_attribute(name) else {
        let p = src.pos(0);
        return Err(Box::new(Diagnostic::error(
            "Missing attribute",
            name.to_owned(),
            Range { start: p, end: p },
        )));
    };
    Ok(Evaluator::new(&src).eval(&attr.value, ctx))
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("variables", &self.variables)
            .field(
                "functions",
                &self.functions.map(|fs| fs.keys().collect::<Vec<_>>()),
            )
            .field("parent", &self.parent)
            .finish()
    }
}
