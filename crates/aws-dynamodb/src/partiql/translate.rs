//! A statement's parts as a classic request's: placeholders for names and values, parameters bound, the `WHERE` as a condition
//! expression, and the key it pins.

use aws_wire::ApiError;
use serde_json::{Map, Value as Json, json};

use super::parse::{self, Expr, Func, Lit, Operand, Path, Step};
use crate::meta::Table;
use crate::req::validation;

/// Names and values for one classic request: `#n0`, `:v0`...
#[derive(Default)]
pub(super) struct Placeholders {
    names: Map<String, Json>,
    values: Map<String, Json>,
}

impl Placeholders {
    pub(super) fn name(&mut self, n: &str) -> String {
        if let Some((k, _)) = self.names.iter().find(|(_, v)| v.as_str() == Some(n)) {
            return k.clone();
        }
        let k = format!("#n{}", self.names.len());
        self.names.insert(k.clone(), json!(n));
        k
    }

    pub(super) fn value(&mut self, v: Json) -> String {
        let k = format!(":v{}", self.values.len());
        self.values.insert(k.clone(), v);
        k
    }

    pub(super) fn path(&mut self, p: &Path) -> String {
        let mut out = String::new();
        for step in &p.0 {
            match step {
                Step::Name(n) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    let k = self.name(n);
                    out.push_str(&k);
                }
                Step::Index(i) => out.push_str(&format!("[{i}]")),
            }
        }
        out
    }

    /// Adds the maps to a request body (only those used: the classic operations refuse an unused one).
    pub(super) fn into_body(self, body: &mut Json) {
        if !self.names.is_empty() {
            crate::req::set(body, "ExpressionAttributeNames", Json::Object(self.names));
        }
        if !self.values.is_empty() {
            crate::req::set(body, "ExpressionAttributeValues", Json::Object(self.values));
        }
    }
}

/// The statement's parameters, bound to its `?`s.
pub(super) struct Params<'a>(pub(super) &'a [Json]);

impl Params<'_> {
    /// A value as an attribute value.
    pub(super) fn value(&self, lit: &Lit) -> Result<Json, ApiError> {
        Ok(match lit {
            Lit::Param(i) => self.0.get(*i).cloned().ok_or_else(|| {
                validation("Number of parameters in request and statement don't match.")
            })?,
            Lit::Str(s) => json!({"S": s}),
            Lit::Num(n) => json!({"N": n}),
            Lit::Bool(b) => json!({"BOOL": b}),
            Lit::Null => json!({"NULL": true}),
            Lit::List(items) => {
                json!({"L": items.iter().map(|i| self.value(i)).collect::<Result<Vec<_>, _>>()?})
            }
            Lit::Map(fields) => json!({"M": self.map(fields)?}),
            Lit::Bag(items) => self.set(items)?,
        })
    }

    pub(super) fn map(&self, fields: &[(String, Lit)]) -> Result<Map<String, Json>, ApiError> {
        let mut m = Map::new();
        for (k, v) in fields {
            if m.insert(k.clone(), self.value(v)?).is_some() {
                return Err(validation(format!(
                    "Duplicate attribute name {k} in the item"
                )));
            }
        }
        Ok(m)
    }

    /// `<<...>>`: a string, number or binary set, all of one kind.
    pub(super) fn set(&self, items: &[Lit]) -> Result<Json, ApiError> {
        let mut kind = None;
        let mut members = Vec::new();
        for i in items {
            let v = self.value(i)?;
            let (k, member) = ["S", "N", "B"]
                .iter()
                .find_map(|k| v.get(*k).map(|m| (*k, m.clone())))
                .ok_or_else(|| validation("A set holds strings, numbers or binaries"))?;
            if kind.is_some_and(|seen| seen != k) {
                return Err(validation("A set holds values of one type"));
            }
            kind = Some(k);
            members.push(member);
        }
        let key = format!("{}S", kind.unwrap_or("S"));
        Ok(json!({ key: members }))
    }

    pub(super) fn operand(&self, ph: &mut Placeholders, o: &Operand) -> Result<String, ApiError> {
        Ok(match o {
            Operand::Path(p) => ph.path(p),
            Operand::Value(v) => ph.value(self.value(v)?),
            Operand::Size(p) => format!("size({})", ph.path(p)),
        })
    }

    /// A `WHERE` as a condition expression.
    pub(super) fn condition(&self, ph: &mut Placeholders, e: &Expr) -> Result<String, ApiError> {
        Ok(match e {
            Expr::And(a, b) => format!(
                "({}) AND ({})",
                self.condition(ph, a)?,
                self.condition(ph, b)?
            ),
            Expr::Or(a, b) => format!(
                "({}) OR ({})",
                self.condition(ph, a)?,
                self.condition(ph, b)?
            ),
            Expr::Not(a) => format!("NOT ({})", self.condition(ph, a)?),
            Expr::Cmp(a, op, b) => format!(
                "{} {} {}",
                self.operand(ph, a)?,
                op.text(),
                self.operand(ph, b)?
            ),
            Expr::Between(a, lo, hi) => format!(
                "{} BETWEEN {} AND {}",
                self.operand(ph, a)?,
                self.operand(ph, lo)?,
                self.operand(ph, hi)?
            ),
            Expr::In(a, list) => {
                let items = list
                    .iter()
                    .map(|o| self.operand(ph, o))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("{} IN ({})", self.operand(ph, a)?, items.join(", "))
            }
            Expr::Func(f, p, v) => {
                let name = match f {
                    Func::BeginsWith => "begins_with",
                    Func::Contains => "contains",
                    Func::AttributeType => "attribute_type",
                };
                format!("{name}({}, {})", ph.path(p), self.operand(ph, v)?)
            }
            Expr::Missing(p, true) => format!("attribute_not_exists({})", ph.path(p)),
            Expr::Missing(p, false) => format!("attribute_exists({})", ph.path(p)),
            Expr::Null(p, is) => {
                let path = ph.path(p);
                let null = ph.value(json!({"S": "NULL"}));
                let test =
                    format!("(attribute_not_exists({path}) OR attribute_type({path}, {null}))");
                if *is { test } else { format!("NOT {test}") }
            }
        })
    }
}

/// The parts of a `WHERE` joined by `AND` at its top.
pub(super) fn conjuncts(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::And(a, b) => {
            let mut out = conjuncts(a);
            out.extend(conjuncts(b));
            out
        }
        other => vec![other],
    }
}

/// The value a conjunct pins an attribute to, if it is `attribute = value` (either way round).
pub(super) fn pinned<'e>(e: &'e Expr, attribute: &str) -> Option<&'e Lit> {
    match e {
        Expr::Cmp(Operand::Path(p), parse::Cmp::Eq, Operand::Value(v))
        | Expr::Cmp(Operand::Value(v), parse::Cmp::Eq, Operand::Path(p))
            if p.attribute() == Some(attribute) =>
        {
            Some(v)
        }
        _ => None,
    }
}

/// The key attributes a `WHERE` pins, each to its value.
pub(super) type Key<'e> = Vec<(&'e str, &'e Lit)>;

/// The key a `WHERE` names, and what else it says. `None` when it does not pin every key attribute.
pub(super) fn split_key<'e>(t: &'e Table, filter: &'e Expr) -> Option<(Key<'e>, Vec<&'e Expr>)> {
    let mut rest = conjuncts(filter);
    let mut key = Vec::new();
    for k in std::iter::once(&t.hash).chain(t.range.as_ref()) {
        let at = rest.iter().position(|e| pinned(e, &k.name).is_some())?;
        let e = rest.remove(at);
        key.push((k.name.as_str(), pinned(e, &k.name)?));
    }
    Some((key, rest))
}

pub(super) fn no_key() -> ApiError {
    validation("Where clause does not contain a mandatory equality on all key attributes")
}

/// A classic request with the caller's options carried over.
pub(super) fn carry(body: &mut Json, from: &Json, members: &[&str]) {
    for m in members {
        if let Some(v) = from.get(*m) {
            crate::req::set(body, m, v.clone());
        }
    }
}

pub(super) fn join_and(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| format!("({p})"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

pub(super) fn read_token(token: &str) -> Result<Json, ApiError> {
    aws_core::base64::decode(token)
        .and_then(|b| serde_json::from_slice::<Json>(&b).ok())
        .filter(Json::is_object)
        .ok_or_else(|| validation("Invalid NextToken"))
}
