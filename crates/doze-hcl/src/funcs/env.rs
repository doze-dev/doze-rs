//! doze's own functions (`internal/config/funcs.go`): `env` reads a host environment variable,
//! `env_of` gathers the `env` maps that blocks publish and refuses two that set the same variable.

use super::{Spec, map, panic_error, str_arg};
use crate::function::{FuncError, Function, Param};
use crate::types::Type;
use crate::value::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// How `env` looks a variable up: `None` when it is not set.
pub type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The process environment (Go's `os.LookupEnv`; a name Go could never find is not set).
pub(super) fn process_env() -> EnvLookup {
    Arc::new(|name: &str| {
        if name.is_empty() || name.contains(['=', '\0']) {
            return None;
        }
        std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
    })
}

/// `env(name, default...)`: the variable, else the first default, else "".
pub(super) struct Env {
    params: [Param; 1],
    var: Param,
    lookup: EnvLookup,
}

impl Env {
    pub(super) fn new(lookup: EnvLookup) -> Env {
        Env {
            params: [Param::new("name", Type::String)],
            var: Param::new("default", Type::String),
            lookup,
        }
    }
}

impl fmt::Debug for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env").finish_non_exhaustive()
    }
}

impl Function for Env {
    fn params(&self) -> &[Param] {
        &self.params
    }

    fn var_param(&self) -> Option<&Param> {
        Some(&self.var)
    }

    fn return_type(&self, _: &[Value]) -> Result<Type, FuncError> {
        Ok(Type::String)
    }

    fn call(&self, args: &[Value], _: &Type) -> Result<Value, FuncError> {
        if let Some(v) = (self.lookup)(str_arg(args, 0)) {
            return Ok(Value::string(&v));
        }
        Ok(args.get(1).cloned().unwrap_or_else(|| Value::string("")))
    }
}

pub(super) fn env_of() -> Spec {
    Spec::new(vec![], Type::map(Type::String), env_of_impl)
        .var(Param::new("blocks", Type::Dynamic))
        .refine(None)
}

fn env_of_impl(args: &[Value], ret: &Type) -> Result<Value, FuncError> {
    if !args.iter().all(Value::is_wholly_known) {
        return Ok(Value::unknown(ret.clone()));
    }
    let mut out: BTreeMap<String, Value> = BTreeMap::new();
    let mut from: BTreeMap<String, String> = BTreeMap::new();
    for (i, a) in args.iter().enumerate() {
        let Value::Object(attrs) = a else {
            return Err(not_a_block(i));
        };
        let Some(env) = attrs.get("env") else {
            return Err(not_a_block(i));
        };
        let name = match attrs.get("name") {
            None => format!("argument {}", i + 1),
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Err(panic_error("value must be a string")),
        };
        // Go iterates whatever `env` is: a set's elements are their own keys; a list's keys are
        // numbers, which panic as names.
        if !env.can_iterate() {
            return Err(panic_error("value is not a map or object"));
        }
        for (k, v) in env.elements() {
            let Some(key) = k.as_str().map(str::to_owned) else {
                return Err(panic_error("value must be a string"));
            };
            if let Some(prev) = from.get(&key) {
                return Err(FuncError::arg(
                    i,
                    format!(
                        "{prev} and {name} both set {key}; write that one by hand, for example env = merge(env_of(module.{prev}), {{ {key} = module.{name}.url }})"
                    ),
                ));
            }
            from.insert(key.clone(), name.clone());
            out.insert(key, v);
        }
    }
    if out.is_empty() {
        return Ok(Value::Map(Type::String, out));
    }
    // Go builds a map of whatever the values are and fails on its type afterwards.
    if out.values().any(|v| v.ty() != Type::String) {
        return Err(panic_error("env_of values must be strings"));
    }
    map(Type::String, out)
}

fn not_a_block(i: usize) -> FuncError {
    FuncError::arg(i, "env_of takes blocks that publish env, such as module.db")
}
