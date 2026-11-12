//! The JSONata dialect: a string `{% expr %}` anywhere in `Arguments`, `Output`, `Assign`, `Items`, `ItemSelector`, a `Condition`
//! or one of the number and time fields is an expression, evaluated by `jlens-core` with `$states` (`input`, `context`, `result`,
//! `errorOutput`), the variables in scope, and the functions AWS adds: `$partition`, `$range`, `$hash`, `$random`, `$uuid` and
//! `$parse`. An evaluation is bounded in time and depth.
//!
//! The engine is the `jsonata` feature (on by default). Built without it, every expression is refused when the definition is
//! checked, with a sentence that says why, so a JSONata machine is never accepted and then left to fail.

#[cfg(feature = "jsonata")]
mod engine;

use serde_json::{Map, Value};

use crate::env::Env;

/// Whether `s` is a whole expression.
#[must_use]
pub fn is_expr(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 4 && t.starts_with("{%") && t.ends_with("%}")
}

pub(crate) fn body(s: &str) -> &str {
    let t = s.trim();
    t.get(2..t.len() - 2).unwrap_or_default().trim()
}

/// Whether this build runs JSONata.
#[must_use]
pub const fn available() -> bool {
    cfg!(feature = "jsonata")
}

/// Why `expr` cannot be run, if it cannot.
fn compile_error(expr: &str) -> Option<String> {
    #[cfg(feature = "jsonata")]
    {
        engine::compile_error(expr)
    }
    #[cfg(not(feature = "jsonata"))]
    {
        let _ = expr;
        Some(
            "JSONata is not built into this doze-aws (it was built without the jsonata feature)"
                .to_owned(),
        )
    }
}

/// The expressions in `v` that do not compile, with where they are.
pub fn problems(v: &Value, at: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::String(s) if is_expr(s) => {
            if let Some(why) = compile_error(body(s)) {
                // (memory)
                out.push((
                    at.to_owned(),
                    format!("The JSONata expression '{}' is not valid: {why}", body(s)),
                ));
            }
        }
        Value::Object(m) => m
            .iter()
            .for_each(|(k, v)| problems(v, &format!("{at}/{k}"), out)),
        Value::Array(a) => a
            .iter()
            .enumerate()
            .for_each(|(i, v)| problems(v, &format!("{at}/{i}"), out)),
        _ => {}
    }
}

/// What an expression sees.
#[derive(Clone, Copy, Debug)]
pub struct Scope<'a> {
    /// `$states`.
    pub states: &'a Value,
    /// The variables in scope, each bound as `$name`.
    pub vars: &'a Map<String, Value>,
}

/// An expression that failed: its text and the engine's reason.
#[derive(Clone, Debug)]
pub struct Failed {
    pub expr: String,
    pub why: String,
}

/// Evaluates every expression in `v` (a template: objects and arrays are walked, other values kept). `None` when the whole of `v` is
/// one expression whose value is undefined; a member whose expression is undefined is left out.
///
/// # Errors
/// The first expression that fails.
pub fn eval(v: &Value, scope: Scope<'_>, env: &Env) -> Result<Option<Value>, Failed> {
    #[cfg(feature = "jsonata")]
    {
        engine::eval(v, scope, env)
    }
    #[cfg(not(feature = "jsonata"))]
    {
        let _ = (scope, env);
        Err(Failed {
            expr: v.to_string(),
            why: compile_error("").unwrap_or_default(),
        })
    }
}
