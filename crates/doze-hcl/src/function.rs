//! Functions, with cty's calling rules (`cty/function`): parameters say whether they take nulls,
//! unknowns and values of not-yet-known type; a call whose arguments are not ready returns an unknown
//! of the result type instead of running.

use crate::types::Type;
use crate::value::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: &'static str,
    pub ty: Type,
    pub allow_null: bool,
    pub allow_unknown: bool,
    pub allow_dynamic: bool,
}

impl Param {
    pub const fn new(name: &'static str, ty: Type) -> Param {
        Param {
            name,
            ty,
            allow_null: false,
            allow_unknown: false,
            allow_dynamic: false,
        }
    }

    pub const fn dynamic(mut self) -> Param {
        self.allow_dynamic = true;
        self
    }

    pub const fn unknown(mut self) -> Param {
        self.allow_unknown = true;
        self
    }

    pub const fn null(mut self) -> Param {
        self.allow_null = true;
        self
    }
}

/// A failed call: which argument (if one is to blame) and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncError {
    pub arg: Option<usize>,
    pub msg: String,
}

impl FuncError {
    pub fn new(msg: impl Into<String>) -> FuncError {
        FuncError {
            arg: None,
            msg: msg.into(),
        }
    }

    pub fn arg(i: usize, msg: impl Into<String>) -> FuncError {
        FuncError {
            arg: Some(i),
            msg: msg.into(),
        }
    }
}

impl fmt::Display for FuncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

pub trait Function: Send + Sync {
    fn params(&self) -> &[Param];

    fn var_param(&self) -> Option<&Param> {
        None
    }

    /// The result type for these arguments (they passed the parameter checks).
    fn return_type(&self, args: &[Value]) -> Result<Type, FuncError>;

    /// Runs with arguments that are known where the parameters asked for known ones.
    fn call(&self, args: &[Value], ret: &Type) -> Result<Value, FuncError>;
}

/// The functions an expression may call, by name.
pub type Functions = BTreeMap<String, Box<dyn Function>>;

/// cty's `Function.Call`.
pub fn call(f: &dyn Function, args: &[Value]) -> Result<Value, FuncError> {
    let (ret, dyn_args) = return_type_for(f, args)?;
    if dyn_args {
        return Ok(Value::Unknown(Type::Dynamic));
    }
    let params = f.params();
    let not_ready = args.iter().enumerate().any(|(i, v)| {
        let p = params.get(i).or_else(|| f.var_param());
        !v.is_known() && !p.is_some_and(|p| p.allow_unknown)
    });
    if not_ready {
        return Ok(Value::Unknown(ret));
    }
    f.call(args, &ret)
}

/// cty's `returnTypeForValues`: arity, nulls, dynamic-typed arguments and conformance, then the
/// function's own type rule. The flag is true when an argument's type is not yet known.
fn return_type_for(f: &dyn Function, args: &[Value]) -> Result<(Type, bool), FuncError> {
    let params = f.params();
    match f.var_param() {
        None if args.len() != params.len() => {
            return Err(FuncError::new(format!(
                "wrong number of arguments ({} required; {} given)",
                params.len(),
                args.len()
            )));
        }
        Some(_) if args.len() < params.len() => {
            return Err(FuncError::new(format!(
                "wrong number of arguments (at least {} required; {} given)",
                params.len(),
                args.len()
            )));
        }
        _ => {}
    }
    for (i, v) in args.iter().enumerate() {
        let (p, err_index) = match params.get(i) {
            Some(p) => (p, i),
            // cty reports a variadic argument's conformance error at its index among the variadic
            // arguments, not among all of them; kept, so diagnostics point where Go's do.
            None => (
                f.var_param()
                    .ok_or_else(|| FuncError::new("too many arguments"))?,
                i - params.len(),
            ),
        };
        if v.is_null() && !p.allow_null {
            return Err(FuncError::arg(i, "argument must not be null"));
        }
        let t = v.ty();
        if t == Type::Dynamic {
            if !p.allow_dynamic {
                return Ok((Type::Dynamic, true));
            }
        } else if !t.conforms_to(&p.ty) {
            return Err(FuncError::arg(
                err_index,
                crate::convert::mismatch(&t, &p.ty),
            ));
        }
    }
    Ok((f.return_type(args)?, false))
}
