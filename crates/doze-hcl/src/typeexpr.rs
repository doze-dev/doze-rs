//! Type expressions (hcl's `ext/typeexpr`): `string`, `list(number)`, `object({a = optional(bool)})`
//! read as a cty type, as doze reads a variable's `type`. Without defaults (doze does not use
//! `optional(type, default)`; Go refuses the second argument in that mode too).

use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::syntax::Source;
use crate::types::{ObjectType, Type};
use hcl_edit::Span;
use hcl_edit::expr::{Expression, FuncCall, ObjectKey};
use std::collections::{BTreeMap, BTreeSet};

const SUMMARY: &str = "Invalid type specification";

/// `typeexpr.TypeConstraint`: `any` is allowed and so is `optional` inside `object`.
pub fn type_constraint(src: &Source, expr: &Expression) -> (Type, Diagnostics) {
    Reader {
        src,
        constraint: true,
    }
    .get(expr)
}

/// `typeexpr.Type`: an exact type, no `any`, no `optional`.
pub fn exact_type(src: &Source, expr: &Expression) -> (Type, Diagnostics) {
    Reader {
        src,
        constraint: false,
    }
    .get(expr)
}

struct Reader<'s> {
    src: &'s Source,
    constraint: bool,
}

fn bad(detail: impl Into<String>, at: Range) -> (Type, Diagnostics) {
    (Type::Dynamic, Diagnostic::error(SUMMARY, detail, at).into())
}

/// `hcl.ExprAsKeyword`: a lone name (or `null`, `true`, `false`).
fn keyword(e: &Expression) -> Option<&str> {
    match e {
        Expression::Variable(v) => Some(v.as_str()),
        Expression::Null(_) => Some("null"),
        Expression::Bool(b) => Some(if *b.value() { "true" } else { "false" }),
        _ => None,
    }
}

/// A call's pieces as `hcl.StaticCall` has them.
struct Call<'e> {
    name: String,
    name_at: Range,
    args: Vec<&'e Expression>,
    args_at: Range,
}

impl Reader<'_> {
    fn range<T: Span + ?Sized>(&self, node: &T) -> Range {
        node.span().map(|s| self.src.range(s)).unwrap_or_default()
    }

    fn call<'e>(&self, f: &'e FuncCall) -> Call<'e> {
        let whole = f.span().unwrap_or_default();
        let name_end = f.name.name.span().map_or(whole.start, |s| s.end);
        let name_start = f
            .name
            .namespace
            .first()
            .and_then(Span::span)
            .map_or(whole.start, |s| s.start);
        let open = self
            .src
            .text()
            .get(name_end..whole.end)
            .and_then(|t| t.find('('))
            .map_or(name_end, |i| name_end + i);
        let name = f
            .name
            .namespace
            .iter()
            .map(|n| format!("{}::", n.as_str()))
            .collect::<String>()
            + f.name.name.as_str();
        Call {
            name,
            name_at: self.src.range(name_start..name_end),
            args: f.args.iter().collect(),
            args_at: self.src.range(open..whole.end),
        }
    }

    fn get(&self, expr: &Expression) -> (Type, Diagnostics) {
        let at = self.range(expr);
        match keyword(expr) {
            Some("bool") => return (Type::Bool, Diagnostics::default()),
            Some("string") => return (Type::String, Diagnostics::default()),
            Some("number") => return (Type::Number, Diagnostics::default()),
            Some("any") if self.constraint => return (Type::Dynamic, Diagnostics::default()),
            Some("any") => {
                return bad(
                    "The keyword \"any\" cannot be used in this type specification: an exact type is required.",
                    at,
                );
            }
            Some(kw @ ("list" | "map" | "set")) => {
                return bad(
                    format!(
                        "The {kw} type constructor requires one argument specifying the element type."
                    ),
                    at,
                );
            }
            Some("object") => {
                return bad(
                    "The object type constructor requires one argument specifying the attribute types and values as a map.",
                    at,
                );
            }
            Some("tuple") => {
                return bad(
                    "The tuple type constructor requires one argument specifying the element types as a list.",
                    at,
                );
            }
            Some(kw) => {
                return bad(
                    format!("The keyword {kw:?} is not a valid type specification."),
                    at,
                );
            }
            None => {}
        }
        let Expression::FuncCall(f) = expr else {
            return bad(
                "A type specification is either a primitive type keyword (bool, number, string) or a complex type constructor call, like list(string).",
                at,
            );
        };
        let call = self.call(f);
        self.constructor(&call, at)
    }

    fn constructor(&self, call: &Call<'_>, at: Range) -> (Type, Diagnostics) {
        let name = call.name.as_str();
        match name {
            "bool" | "string" | "number" => {
                return bad(
                    format!("Primitive type keyword {name:?} does not expect arguments."),
                    call.args_at,
                );
            }
            "any" => {
                return bad(
                    format!("Type constraint keyword {name:?} does not expect arguments."),
                    call.args_at,
                );
            }
            _ => {}
        }
        if call.args.len() != 1 {
            let subject = match (call.args.get(1), call.args.last()) {
                (Some(a), Some(b)) => Range {
                    start: self.range(*a).start,
                    end: self.range(*b).end,
                },
                _ => call.args_at,
            };
            let detail = match name {
                "list" | "set" | "map" => Some(format!("The {name} type constructor requires one argument specifying the element type.")),
                "object" => Some("The object type constructor requires one argument specifying the attribute types and values as a map.".to_owned()),
                "tuple" => Some("The tuple type constructor requires one argument specifying the element types as a list.".to_owned()),
                _ => None,
            };
            if let Some(detail) = detail {
                let d = Diagnostic::error(SUMMARY, detail, subject).with_context(call.args_at);
                return (Type::Dynamic, d.into());
            }
        }
        let first = call.args.first().copied();
        match (name, first) {
            ("list", Some(e)) => wrap(self.get(e), Type::list),
            ("set", Some(e)) => wrap(self.get(e), Type::set),
            ("map", Some(e)) => wrap(self.get(e), Type::map),
            ("object", Some(e)) => self.object(e, at),
            ("tuple", Some(e)) => self.tuple(e, at),
            ("optional", _) => bad(
                format!("Keyword {name:?} is valid only as a modifier for object type attributes."),
                call.name_at,
            ),
            _ => bad(
                format!("Keyword {name:?} is not a valid type constructor."),
                at,
            ),
        }
    }

    fn tuple(&self, list: &Expression, at: Range) -> (Type, Diagnostics) {
        let Expression::Array(items) = list else {
            let d = Diagnostic::error(
                SUMMARY,
                "Tuple type constructor requires a list of element types.",
                self.range(list),
            )
            .with_context(at);
            return (Type::Dynamic, d.into());
        };
        let mut ds = Diagnostics::default();
        let types = items
            .iter()
            .map(|e| {
                let (t, d) = self.get(e);
                ds.extend(d);
                t
            })
            .collect();
        (Type::Tuple(types), ds)
    }

    fn object(&self, map: &Expression, at: Range) -> (Type, Diagnostics) {
        let Expression::Object(items) = map else {
            let d = Diagnostic::error(SUMMARY, "Object type constructor requires a map whose keys are attribute names and whose values are the corresponding attribute types.", self.range(map)).with_context(at);
            return (Type::Dynamic, d.into());
        };
        let mut ds = Diagnostics::default();
        let mut obj = ObjectType {
            attrs: BTreeMap::new(),
            optional: BTreeSet::new(),
        };
        // In source order, with the items a later key redefined (Go sees every item).
        let mut all: Vec<(&ObjectKey, &hcl_edit::expr::ObjectValue)> = items.iter().collect();
        all.extend(items.redefined().iter().map(|(k, v)| (k, v)));
        all.sort_by_key(|(k, _)| k.span().map_or(0, |s| s.start));
        for (key, value) in all {
            let key_at = self.range(key);
            let name = match key {
                ObjectKey::Ident(i) => Some(i.as_str()),
                ObjectKey::Expression(e) => keyword(e),
            };
            let Some(name) = name else {
                ds.push(
                    Diagnostic::error(
                        SUMMARY,
                        "Object constructor map keys must be attribute names.",
                        key_at,
                    )
                    .with_context(at),
                );
                continue;
            };
            if obj.attrs.contains_key(name) {
                ds.push(
                    Diagnostic::error(
                        SUMMARY,
                        "Object constructor map keys must be unique.",
                        key_at,
                    )
                    .with_context(at),
                );
                continue;
            }
            let Some(ty_expr) = self.optional(value.expr(), name, &mut obj.optional, &mut ds)
            else {
                continue;
            };
            let (t, d) = self.get(ty_expr);
            ds.extend(d);
            obj.attrs.insert(name.to_owned(), t);
        }
        (Type::Object(obj), ds)
    }

    /// `optional(type)` around an attribute's type: marks it and returns the type inside.
    /// `None` when the attribute is to be skipped (`optional()` with nothing in it).
    fn optional<'e>(
        &self,
        e: &'e Expression,
        name: &str,
        optional: &mut BTreeSet<String>,
        ds: &mut Diagnostics,
    ) -> Option<&'e Expression> {
        let Expression::FuncCall(f) = e else {
            return Some(e);
        };
        let call = self.call(f);
        if call.name != "optional" {
            return Some(e);
        }
        let at = self.range(e);
        let Some(inner) = call.args.first().copied() else {
            ds.push(
                Diagnostic::error(
                    SUMMARY,
                    "Optional attribute modifier requires the attribute type as its argument.",
                    call.args_at,
                )
                .with_context(at),
            );
            return None;
        };
        if !self.constraint {
            ds.push(Diagnostic::error(SUMMARY, "Optional attribute modifier is only for type constraints, not for exact types.", call.name_at).with_context(at));
        } else if call.args.len() == 1 {
            optional.insert(name.to_owned());
        } else {
            ds.push(
                Diagnostic::error(
                    SUMMARY,
                    "Optional attribute modifier expects only one argument: the attribute type.",
                    call.args_at,
                )
                .with_context(at),
            );
        }
        Some(inner)
    }
}

fn wrap((t, ds): (Type, Diagnostics), f: fn(Type) -> Type) -> (Type, Diagnostics) {
    (f(t), ds)
}
