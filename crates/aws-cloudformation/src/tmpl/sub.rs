//! `Fn::Sub`: `${Name}`, `${Resource.Attribute}` and `${!Literal}` in a string, with an optional map of variables.

use std::collections::BTreeMap;

use serde_json::Value;

use super::eval::{Fault, Scope, err};

/// A piece of a `Fn::Sub` string.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Piece<'a> {
    Text(&'a str),
    /// `${!X}`: written as `${X}`.
    Literal(&'a str),
    Var(&'a str),
}

/// The pieces of a `Fn::Sub` string.
pub(crate) fn pieces(s: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        let Some(len) = rest.get(start + 2..).and_then(|t| t.find('}')) else {
            break;
        };
        if start > 0 {
            out.push(Piece::Text(rest.get(..start).unwrap_or_default()));
        }
        let inner = rest.get(start + 2..start + 2 + len).unwrap_or_default();
        out.push(match inner.strip_prefix('!') {
            Some(lit) => Piece::Literal(lit),
            None => Piece::Var(inner.trim()),
        });
        rest = rest.get(start + 3 + len..).unwrap_or_default();
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

/// The string and the variable map of a `Fn::Sub` argument.
pub(crate) fn parts(arg: &Value) -> Option<(&str, Option<&serde_json::Map<String, Value>>)> {
    match arg {
        Value::String(s) => Some((s, None)),
        Value::Array(a) => match a.as_slice() {
            [Value::String(s)] => Some((s, None)),
            [Value::String(s), Value::Object(m)] => Some((s, Some(m))),
            _ => None,
        },
        _ => None,
    }
}

impl Scope<'_> {
    pub(super) fn sub(&self, arg: &Value) -> Result<String, Fault> {
        let Some((text, vars)) = parts(arg) else {
            // (memory)
            return err(
                "Template error: One or more Fn::Sub intrinsic functions don't specify expected arguments. Specify a string as first argument, and an optional second argument to specify a mapping of values to replace in the string",
            );
        };
        let mut values: BTreeMap<&str, String> = BTreeMap::new();
        if let Some(vars) = vars {
            for (k, v) in vars {
                values.insert(k, self.text(v, "Fn::Sub")?);
            }
        }
        let mut out = String::with_capacity(text.len());
        for piece in pieces(text) {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Literal(l) => {
                    out.push_str("${");
                    out.push_str(l);
                    out.push('}');
                }
                Piece::Var(name) => {
                    if let Some(v) = values.get(name) {
                        out.push_str(v);
                        continue;
                    }
                    out.push_str(&self.sub_var(name)?);
                }
            }
        }
        Ok(out)
    }

    fn sub_var(&self, name: &str) -> Result<String, Fault> {
        let as_text = |v: Value| match v {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            Value::Array(a) => Ok(a
                .iter()
                .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_owned))
                .collect::<Vec<_>>()
                .join(",")),
            // (memory)
            _ => err(format!(
                "Template error: variable {name} in Fn::Sub expression does not resolve to a string"
            )),
        };
        let reference = if !self.params.contains_key(name)
            && !name.starts_with("AWS::")
            && let Some((res, attr)) = name.split_once('.')
        {
            self.attribute(res, attr)?
        } else {
            self.value(&serde_json::json!({ "Ref": name }), "Fn::Sub")?
        };
        as_text(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::{Piece, pieces};

    #[test]
    fn pieces_of_a_sub_string() {
        assert_eq!(
            pieces("a-${X}-${!Y}-${R.Arn}"),
            vec![
                Piece::Text("a-"),
                Piece::Var("X"),
                Piece::Text("-"),
                Piece::Literal("Y"),
                Piece::Text("-"),
                Piece::Var("R.Arn")
            ]
        );
        assert_eq!(pieces("${open"), vec![Piece::Text("${open")]);
    }
}
