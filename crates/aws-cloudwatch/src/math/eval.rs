//! Working out an expression's value over a window: the ids it names are the other queries' results, its functions are those of
//! [`super::funcs`].

use super::parse::Expr;
use super::search::Query;
use super::value::{Ts, Val, combine};

/// Where an expression's names come from.
pub(crate) trait Source {
    /// The value of the query with this id (a metric, or another expression already worked out).
    fn var(&self, id: &str) -> Option<Val>;
    /// The metrics (`MetricStat` queries) of the request, by id, in the order they were given: what `METRICS()` returns.
    fn metrics(&self) -> Vec<(String, Ts)>;
    /// The metrics this region holds that `query` matches, read with `stat` over `period_s`.
    ///
    /// # Errors
    /// Why the search cannot be run.
    fn search(&self, query: &Query, stat: &str, period_s: i64) -> Result<Vec<Ts>, String>;
}

/// What an expression is worked out in.
pub(crate) struct Env<'a> {
    /// The start of every period of the window, oldest first.
    pub(crate) grid: &'a [i64],
    pub(crate) period_ms: i64,
    pub(crate) src: &'a dyn Source,
}

/// Whether `name` is an id (the model: "The first character must be a lowercase letter").
pub(crate) fn is_id(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

/// The value of `e`.
///
/// # Errors
/// What is wrong, in a sentence.
pub(crate) fn eval(e: &Expr, env: &Env<'_>) -> Result<Val, String> {
    match e {
        Expr::Num(n) => Ok(Val::Scalar(*n)),
        Expr::Str(s) => Ok(Val::Text(s.clone())),
        Expr::Name(n) if is_id(n) => env.src.var(n).ok_or_else(|| format!("ID '{n}' not found")),
        Expr::Name(n) => Ok(Val::Word(n.clone())),
        Expr::List(items) => {
            let mut all = Vec::new();
            for i in items {
                match eval(i, env)? {
                    Val::Series(ts) => all.push(ts),
                    Val::Array(more) => all.extend(more),
                    other => return Err(format!("a list holds time series, not {}", other.kind())),
                }
            }
            Ok(Val::Array(all))
        }
        Expr::Neg(x) => eval(x, env)?.map(&|v| -v),
        Expr::Not(x) => eval(x, env)?.map(&|v| if v == 0.0 { 1.0 } else { 0.0 }),
        Expr::Bin(op, a, b) => combine(*op, eval(a, env)?, eval(b, env)?),
        Expr::Call(f, args) => {
            let mut values = Vec::with_capacity(args.len());
            for a in args {
                values.push(eval(a, env)?);
            }
            super::funcs::call(f, values, env)
        }
    }
}
