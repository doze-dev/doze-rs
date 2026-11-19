//! cty's encoding functions (`stdlib/json.go`, `csv.go`) and its JSON encoding of values
//! (`cty/json`'s `Marshal`), which `format`'s `%v` and `%#v` share.

use super::csv::Reader;
use super::print::json_string;
use super::{Spec, arg, list, str_arg};
use crate::function::{FuncError, Param};
use crate::types::Type;
use crate::value::{Refinement, Value};
use std::collections::BTreeMap;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    vec![
        (
            "jsonencode",
            Spec::new(
                vec![Param::new("val", Type::Dynamic).unknown().dynamic().null()],
                Type::String,
                jsonencode,
            ),
        ),
        (
            "jsondecode",
            Spec::rule(
                vec![Param::new("str", Type::String)],
                jsondecode_type,
                jsondecode,
            )
            .refine(None),
        ),
        (
            "csvdecode",
            Spec::rule(
                vec![Param::new("str", Type::String)],
                csvdecode_type,
                csvdecode,
            ),
        ),
    ]
}

type Out = Result<Value, FuncError>;

fn jsonencode(args: &[Value], _: &Type) -> Out {
    let v = arg(args, 0);
    if !v.is_wholly_known() {
        if v.could_be_null() {
            return Ok(Value::unknown(Type::String));
        }
        let prefix = match v.ty() {
            Type::String => "\"",
            Type::Object(_) | Type::Map(_) => "{",
            Type::Tuple(_) | Type::List(_) | Type::Set(_) => "[",
            _ => "",
        };
        let r = Refinement {
            prefix: prefix.to_owned(),
            ..Refinement::default()
        };
        return Ok(Value::refined(Type::String, r));
    }
    if v.is_null() {
        return Ok(Value::string("null"));
    }
    marshal(v)
        .map(|s| Value::string(&s))
        .map_err(FuncError::new)
}

/// cty's JSON for a known value of its own type.
pub(super) fn marshal(v: &Value) -> Result<String, String> {
    let mut out = String::new();
    write(&mut out, v)?;
    Ok(out)
}

fn write(out: &mut String, v: &Value) -> Result<(), String> {
    match v {
        Value::Unknown(..) => return Err("value is not known".to_owned()),
        Value::Null(_) => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) if n.is_inf() => {
            return Err("cannot serialize infinity as JSON".to_owned());
        }
        Value::Number(n) => out.push_str(&n.text()),
        Value::String(s) => json_string(out, s),
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => {
            out.push('[');
            for (i, e) in es.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(out, e)?;
            }
            out.push(']');
        }
        Value::Map(_, m) | Value::Object(m) => {
            out.push('{');
            for (i, (k, e)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_string(out, k);
                out.push(':');
                write(out, e)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// For an unknown string, a type from the first character of its known prefix.
fn jsondecode_type(args: &[Value]) -> Result<Type, FuncError> {
    let v = arg(args, 0);
    let Value::Unknown(_, r) = v else {
        return super::json::decode(str_arg(args, 0))
            .map(|v| v.ty())
            .map_err(FuncError::new);
    };
    let prefix = r.as_ref().map_or("", |r| r.prefix.as_str()).trim();
    let Some(c) = prefix.chars().next() else {
        return Ok(Type::Dynamic);
    };
    match c {
        '{' | '[' | 'n' => Ok(Type::Dynamic),
        '"' => Ok(Type::String),
        't' | 'f' => Ok(Type::Bool),
        '-' | '0'..='9' | '.' => Ok(Type::Number),
        c => Err(FuncError::arg(
            0,
            format!("a JSON document cannot begin with the character {c:?}"),
        )),
    }
}

fn jsondecode(args: &[Value], _: &Type) -> Out {
    super::json::decode(str_arg(args, 0)).map_err(FuncError::new)
}

fn csv_error(e: &super::csv::CsvError) -> FuncError {
    FuncError::new(format!("CSV parse error on line {}: {}", e.line, e.msg))
}

/// A list of objects, one attribute per column of the header line.
fn csvdecode_type(args: &[Value]) -> Result<Type, FuncError> {
    if !arg(args, 0).is_known() {
        return Ok(Type::Dynamic);
    }
    let mut r = Reader::new(str_arg(args, 0));
    let headers = match r.read() {
        None => return Err(FuncError::new("missing header line")),
        Some(Err(e)) => return Err(csv_error(&e)),
        Some(Ok(h)) => h,
    };
    let mut attrs = BTreeMap::new();
    for h in headers {
        let name = Value::string(&h).as_str().unwrap_or("").to_owned();
        if attrs.insert(name, Type::String).is_some() {
            return Err(FuncError::new(format!(
                "duplicate column name {}",
                crate::value::quote::go_quote(&h)
            )));
        }
    }
    Ok(Type::list(Type::object(attrs)))
}

fn csvdecode(args: &[Value], ret: &Type) -> Out {
    let ety = ret.element().cloned().unwrap_or(Type::Dynamic);
    let mut r = Reader::new(str_arg(args, 0));
    let headers = match r.read() {
        Some(Ok(h)) => h,
        _ => return Err(FuncError::new("missing header line")),
    };
    let mut rows = Vec::new();
    while let Some(rec) = r.read() {
        let cols = rec.map_err(|e| csv_error(&e))?;
        let row = headers
            .iter()
            .zip(cols)
            .map(|(h, c)| {
                (
                    Value::string(h).as_str().unwrap_or("").to_owned(),
                    Value::string(&c),
                )
            })
            .collect();
        rows.push(Value::Object(row));
    }
    if rows.is_empty() {
        return Ok(Value::List(ety, Vec::new()));
    }
    list(ety, rows)
}
