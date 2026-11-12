//! The intrinsic functions of the JSONPath dialect: `States.Format`, `States.StringToJson`, `States.JsonToString`, `States.Array`,
//! `States.ArrayPartition`, `States.ArrayContains`, `States.ArrayRange`, `States.ArrayGetItem`, `States.ArrayLength`,
//! `States.ArrayUnique`, `States.Base64Encode`, `States.Base64Decode`, `States.Hash`, `States.JsonMerge`, `States.MathRandom`,
//! `States.MathAdd`, `States.StringSplit` and `States.UUID`. A call that cannot run fails with `States.IntrinsicFailure`.

mod fns;
mod parse;

pub use parse::{Arg, Call, parse};

use serde_json::Value;

use crate::env::Env;
use crate::errors::{Failure, INTRINSIC_FAILURE};
use crate::path::Scope;

/// Runs `text` (a whole intrinsic call).
///
/// # Errors
/// `States.IntrinsicFailure`, or the failure of a path an argument names.
pub fn eval(text: &str, scope: Scope<'_>, env: &Env) -> Result<Value, Failure> {
    let call = parse(text).map_err(|e| Failure::new(INTRINSIC_FAILURE, e))?;
    run(&call, scope, env)
}

fn run(call: &Call, scope: Scope<'_>, env: &Env) -> Result<Value, Failure> {
    let mut args = Vec::with_capacity(call.args.len());
    for a in &call.args {
        args.push(match a {
            Arg::Literal(v) => v.clone(),
            Arg::Path(p) => p.select(scope).ok_or_else(|| {
                Failure::new(
                    INTRINSIC_FAILURE,
                    format!(
                        "The JsonPath argument for the field '{}' could not be found in the input",
                        p.text
                    ),
                )
            })?,
            Arg::Call(inner) => run(inner, scope, env)?,
        });
    }
    fns::apply(&call.name, &args, env).map_err(|why| Failure::new(INTRINSIC_FAILURE, why))
}

/// Runs the function `name` on arguments already evaluated (JSONata's `$hash`, `$partition` and `$range` are these functions).
pub(crate) fn apply(name: &str, args: &[Value], env: &Env) -> Result<Value, String> {
    fns::apply(name, args, env)
}

/// Whether `text` reads as an intrinsic call (for the definition checks).
#[must_use]
pub fn is_call(text: &str) -> bool {
    parse(text).is_ok_and(|c| fns::known(&c))
}
