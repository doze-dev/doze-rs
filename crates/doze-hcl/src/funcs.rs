//! doze.hcl's function library: the go-cty standard library functions doze's `internal/config`
//! table names (`stdlib.UpperFunc` and the rest), the `to*` conversions and doze's own `env` and
//! `env_of`, each with cty's parameter rules, return types, refinements and errors.
//!
//! Every function is checked against Go by the oracle corpus `testdata/eval/exprs-funcs.json`.

mod case;
mod collection;
mod convert;
mod csv;
mod encoding;
mod env;
mod format;
mod graphemes;
mod json;
mod numeric;
mod print;
mod regex;
mod regex_class;
mod regex_names;
mod regex_syntax;
mod sequence;
mod string;

pub use env::EnvLookup;

use crate::function::{FuncError, Function, Functions, Param};
use crate::number::Number;
use crate::types::Type;
use crate::value::{Refinement, Value};
use std::sync::Arc;

/// doze.hcl's functions, with `env` reading the process environment.
pub fn doze_functions() -> Functions {
    doze_functions_with_env(env::process_env())
}

/// doze.hcl's functions, with `env` reading through `lookup` (tests give it Go's oracle
/// environment, where only `PATH` is set).
pub fn doze_functions_with_env(lookup: EnvLookup) -> Functions {
    let mut fs = Functions::new();
    let mut add = |name: &str, f: Box<dyn Function>| {
        fs.insert(name.to_owned(), f);
    };
    for (name, spec) in string::functions()
        .into_iter()
        .chain(format::functions())
        .chain(regex::functions())
        .chain(collection::functions())
        .chain(sequence::functions())
        .chain(numeric::functions())
        .chain(encoding::functions())
        .chain(convert::functions())
    {
        add(name, Box::new(spec));
    }
    add("env", Box::new(env::Env::new(Arc::clone(&lookup))));
    add("env_of", Box::new(env::env_of()));
    fs
}

/// How a function finds its return type.
#[derive(Debug, Clone)]
pub(crate) enum Ret {
    Static(Type),
    Rule(fn(&[Value]) -> Result<Type, FuncError>),
}

type Impl = fn(&[Value], &Type) -> Result<Value, FuncError>;

/// A function as cty's `function.Spec` describes one.
#[derive(Debug, Clone)]
pub(crate) struct Spec {
    params: Vec<Param>,
    var: Option<Param>,
    ret: Ret,
    imp: Impl,
    refine: Option<Refinement>,
}

impl Spec {
    /// A function with these parameters, a static return type and `RefineResult: refineNonNull`
    /// (as most of cty's are).
    pub(crate) fn new(params: Vec<Param>, ret: Type, imp: Impl) -> Spec {
        Spec {
            params,
            var: None,
            ret: Ret::Static(ret),
            imp,
            refine: Some(Refinement::not_null()),
        }
    }

    /// The same, with a type rule.
    pub(crate) fn rule(
        params: Vec<Param>,
        rule: fn(&[Value]) -> Result<Type, FuncError>,
        imp: Impl,
    ) -> Spec {
        Spec {
            ret: Ret::Rule(rule),
            ..Spec::new(params, Type::Dynamic, imp)
        }
    }

    pub(crate) fn var(mut self, p: Param) -> Spec {
        self.var = Some(p);
        self
    }

    pub(crate) fn refine(mut self, r: Option<Refinement>) -> Spec {
        self.refine = r;
        self
    }
}

impl Function for Spec {
    fn params(&self) -> &[Param] {
        &self.params
    }

    fn var_param(&self) -> Option<&Param> {
        self.var.as_ref()
    }

    fn return_type(&self, args: &[Value]) -> Result<Type, FuncError> {
        match &self.ret {
            Ret::Static(t) => Ok(t.clone()),
            Ret::Rule(f) => f(args),
        }
    }

    fn call(&self, args: &[Value], ret: &Type) -> Result<Value, FuncError> {
        (self.imp)(args, ret)
    }

    fn refine_result(&self) -> Option<Refinement> {
        self.refine.clone()
    }
}

/// Argument `i`, or the dynamic unknown where there is none (arity is checked before any call).
pub(crate) fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::DYNAMIC)
}

/// Argument `i` as a string ("" where it is not one; parameters typed `string` always are).
pub(crate) fn str_arg(args: &[Value], i: usize) -> &str {
    arg(args, i).as_str().unwrap_or("")
}

/// The words `gocty.FromCtyValue` uses when a number does not fit Go's `int`.
pub(crate) const NOT_AN_INT: &str =
    "value must be a whole number, between -9223372036854775808 and 9223372036854775807";

/// A number as Go's `int` (gocty's conversion): whole and in range.
pub(crate) fn to_int(v: &Value) -> Result<i64, String> {
    v.as_number()
        .and_then(Number::to_i64)
        .ok_or_else(|| NOT_AN_INT.to_owned())
}

/// What cty reports when a Go panic inside a function is recovered (`errorForPanic`).
pub(crate) fn panic_error(what: &str) -> FuncError {
    FuncError::new(format!("panic in function implementation: {what}"))
}

/// cty's `ListVal`: elements that must share one type (Go panics, and cty reports it, if not).
pub(crate) fn list(ety: Type, elems: Vec<Value>) -> Result<Value, FuncError> {
    Value::list(ety, elems).map_err(|_| panic_error("inconsistent list element types"))
}

/// cty's `MapVal`.
pub(crate) fn map(
    ety: Type,
    elems: std::collections::BTreeMap<String, Value>,
) -> Result<Value, FuncError> {
    Value::map(ety, elems).map_err(|_| panic_error("inconsistent map element types"))
}

/// The `NotNull` refinement plus a lower bound on a number.
pub(crate) fn at_least(n: i64) -> Refinement {
    Refinement {
        lower: Some((Number::from_i64(n), true)),
        ..Refinement::not_null()
    }
}
