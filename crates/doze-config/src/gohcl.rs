//! gohcl's `DecodeBody` for doze's Go structs: a body read against the fields' implied schema,
//! each attribute decoded into its Go type, at most one of each pointer block. Every diagnostic is
//! kept (Go's order is a map's, so the oracle compares them as a set).

use crate::body::{Block, Body};
use crate::eval::{decode, decode_bool, decode_int, decode_string, eval};
use doze_hcl::Value;
use doze_hcl::decode::{Attr, Schema};
use doze_hcl::diag::{Diagnostic, Diagnostics};
use doze_hcl::eval::Context;
use doze_hcl::types::Type;
use std::collections::BTreeMap;

/// A field's Go type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Str,
    Bool,
    Int,
    StrList,
    StrMap,
    /// `cty.Value`: the value as it evaluates.
    Any,
    /// `hcl.Expression`: not evaluated.
    Expr,
}

/// `hcl:"name"` or `hcl:"name,optional"`.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
}

pub(crate) const fn opt(name: &'static str, ty: Ty) -> Field {
    Field {
        name,
        ty,
        required: false,
    }
}

pub(crate) const fn req(name: &'static str, ty: Ty) -> Field {
    Field {
        name,
        ty,
        required: true,
    }
}

/// A Go struct: its attributes, its optional single blocks (`*T` with `hcl:"x,block"`), and
/// whether it keeps the rest (`hcl:",remain"`).
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub attrs: &'static [Field],
    pub blocks: &'static [(&'static str, &'static Spec)],
    pub remain: bool,
}

/// A decoded field.
#[derive(Debug, Clone)]
pub(crate) enum Got<'a> {
    Str(String),
    Bool(bool),
    Int(i64),
    List(Vec<String>),
    Map(BTreeMap<String, String>),
    Any(Value),
    Expr(Attr<'a>),
}

/// A decoded struct. A field that is unset or failed is absent (Go's zero value).
#[derive(Debug, Clone, Default)]
pub(crate) struct Decoded<'a> {
    pub fields: BTreeMap<&'static str, Got<'a>>,
    pub blocks: BTreeMap<&'static str, Decoded<'a>>,
    pub remain: Option<Body<'a>>,
}

impl<'a> Decoded<'a> {
    pub(crate) fn str(&self, name: &str) -> String {
        match self.fields.get(name) {
            Some(Got::Str(s)) => s.clone(),
            _ => String::new(),
        }
    }
    pub(crate) fn bool(&self, name: &str) -> bool {
        matches!(self.fields.get(name), Some(Got::Bool(true)))
    }
    pub(crate) fn int(&self, name: &str) -> i64 {
        match self.fields.get(name) {
            Some(Got::Int(n)) => *n,
            _ => 0,
        }
    }
    pub(crate) fn list(&self, name: &str) -> Vec<String> {
        match self.fields.get(name) {
            Some(Got::List(l)) => l.clone(),
            _ => Vec::new(),
        }
    }
    pub(crate) fn map(&self, name: &str) -> BTreeMap<String, String> {
        match self.fields.get(name) {
            Some(Got::Map(m)) => m.clone(),
            _ => BTreeMap::new(),
        }
    }
    /// A `cty.Value` field: null when unset.
    pub(crate) fn any(&self, name: &str) -> Value {
        match self.fields.get(name) {
            Some(Got::Any(v)) => v.clone(),
            _ => Value::Null(Type::Dynamic),
        }
    }
    pub(crate) fn expr(&self, name: &str) -> Option<&Attr<'a>> {
        match self.fields.get(name) {
            Some(Got::Expr(a)) => Some(a),
            _ => None,
        }
    }
}

fn schema_of(spec: &Spec) -> Schema {
    let s = spec
        .attrs
        .iter()
        .fold(Schema::default(), |s, f| s.attr(f.name, f.required));
    spec.blocks.iter().fold(s, |s, (b, _)| s.block(b, &[]))
}

/// gohcl's `DecodeBody`.
pub(crate) fn decode_body<'a>(
    body: &Body<'a>,
    ctx: &Context<'_>,
    spec: &Spec,
) -> (Decoded<'a>, Diagnostics) {
    let schema = schema_of(spec);
    let (content, mut ds, remain) = if spec.remain {
        let (c, rest, ds) = body.partial_content(&schema);
        (c, ds, Some(rest))
    } else {
        let (c, ds) = body.content(&schema);
        (c, ds, None)
    };
    let mut out = Decoded {
        remain,
        ..Decoded::default()
    };
    for f in spec.attrs {
        let Some(attr) = content.attr(f.name) else {
            continue;
        };
        match field(attr, ctx, f.ty) {
            Ok(g) => {
                out.fields.insert(f.name, g);
            }
            Err(d) => ds.extend(d),
        }
    }
    for (ty, inner) in spec.blocks {
        let found: Vec<&Block<'a>> = content.blocks.iter().filter(|b| b.ty == *ty).collect();
        match found.as_slice() {
            [] => {}
            [one] => {
                let (d, bds) = decode_body(&one.body, ctx, inner);
                ds.extend(bds);
                out.blocks.insert(ty, d);
            }
            [first, second, ..] => ds.push(
                Diagnostic::error(
                    format!("Duplicate {ty} block"),
                    format!(
                        "Only one {ty} block is allowed. Another was defined at {}.",
                        first.def
                    ),
                    second.def.range,
                )
                .in_file(&second.def.file),
            ),
        }
    }
    (out, ds)
}

fn field<'a>(attr: &Attr<'a>, ctx: &Context<'_>, ty: Ty) -> Result<Got<'a>, Diagnostics> {
    Ok(match ty {
        Ty::Str => Got::Str(decode_string(attr, ctx)?),
        Ty::Bool => Got::Bool(decode_bool(attr, ctx)?),
        Ty::Int => Got::Int(decode_int(attr, ctx)?),
        Ty::StrList => {
            let v = checked(decode(attr, ctx, &Type::list(Type::String), true))?;
            Got::List(strings(&v))
        }
        Ty::StrMap => {
            let v = checked(decode(attr, ctx, &Type::Map(Box::new(Type::String)), true))?;
            match v {
                Value::Map(_, m) => Got::Map(
                    m.into_iter()
                        .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_owned()))
                        .collect(),
                ),
                _ => Got::Map(BTreeMap::new()),
            }
        }
        Ty::Any => Got::Any(checked(eval(attr, ctx))?),
        Ty::Expr => Got::Expr(attr.clone()),
    })
}

fn checked((v, ds): (Value, Diagnostics)) -> Result<Value, Diagnostics> {
    if ds.has_errors() { Err(ds) } else { Ok(v) }
}

/// A list of strings' elements (`[]string`).
pub(crate) fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => es
            .iter()
            .map(|e| e.as_str().unwrap_or_default().to_owned())
            .collect(),
        _ => Vec::new(),
    }
}
