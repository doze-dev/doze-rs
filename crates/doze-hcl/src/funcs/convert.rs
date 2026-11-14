//! doze's `to*` conversions (`internal/config/funcs.go` `makeToFunc`): convert the argument to the
//! wanted type with cty's unsafe conversion, inferring element types where the wanted type leaves
//! them dynamic.

use super::{Spec, arg};
use crate::convert::{convert, exists};
use crate::function::{FuncError, Param};
use crate::types::Type;
use crate::value::Value;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    let to = |imp: fn(&[Value]) -> Result<Type, FuncError>| {
        Spec::rule(
            vec![Param::new("v", Type::Dynamic).null().dynamic()],
            imp,
            to_impl,
        )
        .refine(None)
    };
    vec![
        ("toset", to(|a| to_type(a, &Type::set(Type::Dynamic)))),
        ("tolist", to(|a| to_type(a, &Type::list(Type::Dynamic)))),
        ("tomap", to(|a| to_type(a, &Type::map(Type::Dynamic)))),
        ("tostring", to(|a| to_type(a, &Type::String))),
        ("tonumber", to(|a| to_type(a, &Type::Number))),
        ("tobool", to(|a| to_type(a, &Type::Bool))),
    ]
}

fn to_type(args: &[Value], want: &Type) -> Result<Type, FuncError> {
    let got = arg(args, 0).ty();
    if got == *want || exists(&got, want, true) {
        return Ok(want.clone());
    }
    Err(FuncError::arg(
        0,
        format!(
            "cannot convert {} to {}",
            got.friendly_name(),
            constraint_name(want)
        ),
    ))
}

/// cty's `FriendlyNameForConstraint`: a dynamic element type reads "any".
fn constraint_name(t: &Type) -> String {
    match t {
        Type::Dynamic => "any type".to_owned(),
        Type::List(e) if **e == Type::Dynamic => "list of any single type".to_owned(),
        Type::Set(e) if **e == Type::Dynamic => "set of any single type".to_owned(),
        Type::Map(e) if **e == Type::Dynamic => "map of any single type".to_owned(),
        t => t.friendly_name(),
    }
}

fn to_impl(args: &[Value], ret: &Type) -> Result<Value, FuncError> {
    convert(arg(args, 0), ret).map_err(|e| FuncError::new(e.0))
}
