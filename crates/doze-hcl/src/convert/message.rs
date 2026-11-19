//! Why a type does not convert (cty's `MismatchMessage`).

use crate::types::Type;
use crate::value::quote::go_quote;

/// The message for `got` not converting to `want`.
pub fn mismatch(got: &Type, want: &Type) -> String {
    match (got, want) {
        (Type::Object(g), Type::Object(w)) => {
            let mut missing: Vec<&String> = w
                .attrs
                .keys()
                .filter(|k| !g.attrs.contains_key(*k) && !w.optional.contains(*k))
                .collect();
            missing.sort();
            match missing.as_slice() {
                [] => {}
                [one] => return format!("attribute {} is required", go_quote(one)),
                [a, b] => {
                    return format!(
                        "attributes {} and {} are required",
                        go_quote(a),
                        go_quote(b)
                    );
                }
                [rest @ .., last] => {
                    let head: Vec<String> = rest.iter().map(|k| go_quote(k)).collect();
                    return format!(
                        "attributes {}, and {} are required",
                        head.join(", "),
                        go_quote(last)
                    );
                }
            }
            for (k, wt) in &w.attrs {
                if let Some(gt) = g.attrs.get(k)
                    && gt != wt
                    && !super::exists(gt, wt, true)
                {
                    return format!("attribute {}: {}", go_quote(k), mismatch(gt, wt));
                }
            }
            "incorrect object attributes".into()
        }
        (Type::Tuple(_), Type::List(e)) if **e == Type::Dynamic => {
            "all list elements must have the same type".into()
        }
        (Type::Tuple(_), Type::Set(e)) if **e == Type::Dynamic => {
            "all set elements must have the same type".into()
        }
        (Type::Object(_), Type::Map(e)) if **e == Type::Dynamic => {
            "all map elements must have the same type".into()
        }
        (Type::Tuple(ts), Type::List(e) | Type::Set(e)) => {
            elements(ts.iter().enumerate().map(|(i, t)| (i.to_string(), t)), e)
        }
        (Type::Object(o), Type::Map(e)) => {
            elements(o.attrs.iter().map(|(k, t)| (go_quote(k), t)), e)
        }
        (g, w) if g.is_collection() && w.is_collection() => collections(g, w),
        (g, w) if confusing(g, w) => format!("{} required", constraint_name(w)),
        (g, w) => format!(
            "{} required, but have {}",
            w.friendly_name(),
            g.friendly_name()
        ),
    }
}

fn elements<'a>(items: impl Iterator<Item = (String, &'a Type)>, want: &Type) -> String {
    for (name, t) in items {
        if t != want && !super::exists(t, want, true) {
            return format!("element {name}: {}", mismatch(t, want));
        }
    }
    format!("all elements must be {}", constraint_name(want))
}

fn collections(got: &Type, want: &Type) -> String {
    let same_kind = matches!(
        (got, want),
        (Type::List(_) | Type::Set(_), Type::List(_) | Type::Set(_)) | (Type::Map(_), Type::Map(_))
    );
    if !same_kind {
        return format!("{} required", constraint_name(want));
    }
    let noun = match want {
        Type::List(_) => "list element type",
        Type::Set(_) => "set element type",
        _ => "map element type",
    };
    match (got.element(), want.element()) {
        (Some(g), Some(w)) => format!("incorrect {noun}: {}", mismatch(g, w)),
        _ => format!("{} required", constraint_name(want)),
    }
}

/// Pairs where "X required, but have Y" would mislead (cty's `typesAreLikelyToCauseConfusion`).
fn confusing(got: &Type, want: &Type) -> bool {
    if got.is_primitive() != want.is_primitive() || *want == Type::String {
        return false;
    }
    !matches!(
        (got, want),
        (Type::Number, Type::Bool) | (Type::Bool, Type::Number)
    )
}

/// cty's `FriendlyNameForConstraint`: `Dynamic` reads "any type".
fn constraint_name(t: &Type) -> String {
    match t {
        Type::Dynamic => "any type".into(),
        Type::List(e) => format!("list of {}", constraint_name(e)),
        Type::Set(e) => format!("set of {}", constraint_name(e)),
        Type::Map(e) => format!("map of {}", constraint_name(e)),
        t => t.friendly_name(),
    }
}
