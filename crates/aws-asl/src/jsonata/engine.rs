//! The JSONata engine: `jlens-core`, with `$states`, the variables in scope and the functions AWS adds. Built with the `jsonata`
//! feature.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use jlens_core::{Bindings, Expression, HostFunction, Options};
use serde_json::{Map, Value};

use super::{Failed, Scope, body, is_expr};
use crate::env::Env;

/// How long one expression may run.
const TIME_LIMIT: Duration = Duration::from_secs(5);

/// Why `expr` does not compile, if it does not.
pub(super) fn compile_error(expr: &str) -> Option<String> {
    Expression::compile(expr).err().map(|e| e.message())
}

pub(super) fn eval(v: &Value, scope: Scope<'_>, env: &Env) -> Result<Option<Value>, Failed> {
    let bindings = bindings(scope, env);
    walk(v, &bindings, env)
}

fn walk(v: &Value, b: &Bindings, env: &Env) -> Result<Option<Value>, Failed> {
    match v {
        Value::String(s) if is_expr(s) => one(body(s), b, env),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                if let Some(x) = walk(v, b, env)? {
                    out.insert(k.clone(), x);
                }
            }
            Ok(Some(Value::Object(out)))
        }
        Value::Array(a) => {
            let mut out = Vec::with_capacity(a.len());
            for v in a {
                out.extend(walk(v, b, env)?);
            }
            Ok(Some(Value::Array(out)))
        }
        other => Ok(Some(other.clone())),
    }
}

fn one(expr: &str, b: &Bindings, env: &Env) -> Result<Option<Value>, Failed> {
    let fail = |e: jlens_core::Error| Failed {
        expr: expr.to_owned(),
        why: format!("{}: {}", e.code(), e.message()),
    };
    let compiled = Expression::compile(expr).map_err(fail)?;
    let options = Options::new()
        .now(env.now_ms)
        .seed(env.next_u64())
        .timeout(TIME_LIMIT)
        .max_depth(2_000);
    compiled.evaluate_with(None, b, &options).map_err(fail)
}

fn bindings(scope: Scope<'_>, env: &Env) -> Bindings {
    let mut b = Bindings::new().var("states", scope.states.clone());
    for (k, v) in scope.vars {
        b = b.var(k.clone(), v.clone());
    }
    let seed = Arc::new(AtomicU64::new(env.next_u64() | 1));
    for (name, f) in functions(seed) {
        b = b.function(name, f);
    }
    b
}

fn arg(args: &[Option<Value>], i: usize) -> Value {
    args.get(i).cloned().flatten().unwrap_or(Value::Null)
}

fn failed(code: &str, why: &str) -> jlens_core::Error {
    jlens_core::Error::new(code).with("message", why)
}

/// The functions AWS adds to JSONata, built on the intrinsic functions where they are the same.
fn functions(seed: Arc<AtomicU64>) -> Vec<(&'static str, HostFunction)> {
    let intrinsic = |name: &'static str| {
        move |args: &[Option<Value>], _: &jlens_core::HostCall<'_>| {
            let all: Vec<Value> = (0..args.len()).map(|i| arg(args, i)).collect();
            let env = Env::new(0, 1);
            crate::intrinsic::apply(name, &all, &env)
                .map(Some)
                .map_err(|why| failed("D3137", &why))
        }
    };
    let next = move || {
        let mut x = seed.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        seed.store(x, Ordering::Relaxed);
        x
    };
    let next_uuid = next.clone();
    vec![
        (
            "partition",
            HostFunction::new(2, intrinsic("States.ArrayPartition")),
        ),
        (
            "range",
            HostFunction::new(3, intrinsic("States.ArrayRange")),
        ),
        ("hash", HostFunction::new(2, intrinsic("States.Hash"))),
        (
            "parse",
            HostFunction::new(1, |args, _| {
                let text = arg(args, 0);
                let text = text.as_str().unwrap_or_default();
                serde_json::from_str(text)
                    .map(Some)
                    .map_err(|_| failed("D3137", "The argument of $parse is not JSON"))
            }),
        ),
        (
            "uuid",
            HostFunction::new(0, move |_, _| {
                Ok(Some(Value::String(Env::new(0, next_uuid()).uuid())))
            }),
        ),
        (
            "random",
            HostFunction::new(1, move |args, _| {
                let env = match arg(args, 0).as_u64() {
                    Some(s) => Env::new(0, s),
                    None => Env::new(0, next()),
                };
                Ok(Some(serde_json::json!(env.unit())))
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use serde_json::Map;

    use super::super::{Scope, eval, is_expr, problems};
    use crate::env::Env;

    #[test]
    fn templates_evaluate() {
        let states = json!({"input": {"a": 2, "items": [1, 2, 3]}});
        let vars = Map::from_iter([("x".to_owned(), json!(10))]);
        let scope = Scope {
            states: &states,
            vars: &vars,
        };
        let env = Env::new(0, 3);
        let t = json!({"sum": "{% $sum($states.input.items) + $x %}", "kept": 1, "gone": "{% $states.input.nope %}"});
        assert_eq!(
            eval(&t, scope, &env).unwrap(),
            Some(json!({"sum": 16, "kept": 1}))
        );
        assert_eq!(
            eval(&json!("{% $partition([1,2,3], 2) %}"), scope, &env).unwrap(),
            Some(json!([[1, 2], [3]]))
        );
        assert_eq!(
            eval(&json!("{% $parse('{\"k\": 1}') %}"), scope, &env).unwrap(),
            Some(json!({"k": 1}))
        );
        assert_eq!(
            eval(&json!("{% $range(0, 4, 2) %}"), scope, &env).unwrap(),
            Some(json!([0, 2, 4]))
        );
        assert_eq!(
            eval(&json!("{% $uuid() %}"), scope, &env)
                .unwrap()
                .unwrap()
                .as_str()
                .unwrap()
                .len(),
            36
        );
        assert!(eval(&json!("{% $states.input.a + 'x' %}"), scope, &env).is_err());
        let mut found = Vec::new();
        problems(&json!({"a": "{% ( %}"}), "/x", &mut found);
        assert_eq!(found.len(), 1);
        assert!(is_expr(" {% 1 %} "));
        assert!(!is_expr("{%"));
    }
}
