//! Choice rules: comparisons (every operator of the language, and its `...Path` twin), `And`, `Or`, `Not`, and JSONata's
//! `Condition`.

use serde_json::Value;

use super::fields::{Obj, SCHEMA};
use super::{Diagnostic, Kind, Lang};

/// One rule of `Choices`: a test, and where it leads (only a top-level rule leads anywhere).
#[derive(Clone, Debug)]
pub struct Rule {
    pub test: Test,
    pub next: Option<String>,
    pub assign: Option<Value>,
    pub output: Option<Value>,
}

/// What a rule tests.
#[derive(Clone, Debug)]
pub enum Test {
    And(Vec<Test>),
    Or(Vec<Test>),
    Not(Box<Test>),
    Compare {
        variable: String,
        op: CmpOp,
        operand: Operand,
    },
    /// JSONata: a boolean, or an expression.
    Condition(Value),
}

/// What a comparison compares with.
#[derive(Clone, Debug)]
pub enum Operand {
    Value(Value),
    Path(String),
}

/// The kind of values an operator compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpKind {
    String,
    Numeric,
    Boolean,
    Timestamp,
    /// `IsNull`, `IsPresent`, `IsNumeric`, `IsString`, `IsBoolean`, `IsTimestamp`: a test of the variable itself.
    Is,
}

/// An operator: its kind and its relation (`Equals`, `LessThan`, ..., `Matches`, or the `Is...` test's name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmpOp {
    pub kind: CmpKind,
    pub relation: &'static str,
}

const RELATIONS: &[&str] = &[
    "Equals",
    "LessThan",
    "GreaterThan",
    "LessThanEquals",
    "GreaterThanEquals",
];

const IS: &[&str] = &[
    "Null",
    "Present",
    "Numeric",
    "String",
    "Boolean",
    "Timestamp",
];

/// The operator a member names, and whether it is the `...Path` form.
fn operator(member: &str) -> Option<(CmpOp, bool)> {
    let (base, path) = member
        .strip_suffix("Path")
        .map_or((member, false), |b| (b, true));
    if base == "StringMatches" && !path {
        return Some((
            CmpOp {
                kind: CmpKind::String,
                relation: "Matches",
            },
            false,
        ));
    }
    if let Some(is) = member.strip_prefix("Is") {
        return IS.iter().find(|n| **n == is).map(|n| {
            (
                CmpOp {
                    kind: CmpKind::Is,
                    relation: n,
                },
                false,
            )
        });
    }
    let kinds = [
        ("String", CmpKind::String),
        ("Numeric", CmpKind::Numeric),
        ("Boolean", CmpKind::Boolean),
        ("Timestamp", CmpKind::Timestamp),
    ];
    for (prefix, kind) in kinds {
        let Some(rel) = base.strip_prefix(prefix) else {
            continue;
        };
        if kind == CmpKind::Boolean && rel != "Equals" {
            return None;
        }
        return RELATIONS
            .iter()
            .find(|r| **r == rel)
            .map(|r| (CmpOp { kind, relation: r }, path));
    }
    None
}

pub(super) fn choice(obj: &Obj<'_>, lang: Lang, out: &mut Vec<Diagnostic>) -> Kind {
    obj.require(&["Choices"], out);
    let mut rules = Vec::new();
    for (i, r) in obj.array("Choices", out).into_iter().flatten().enumerate() {
        let Value::Object(m) = r else { continue };
        let o = Obj::new(m, format!("{}/Choices/{i}", obj.at));
        if let Some(test) = test(&o, lang, true, 0, out) {
            rules.push(Rule {
                test,
                next: o.text("Next", out),
                assign: o.get("Assign").cloned(),
                output: o.get("Output").cloned(),
            });
        }
        if o.get("Next").is_none() {
            o.require(&["Next"], out);
        }
    }
    Kind::Choice {
        rules,
        default: obj.text("Default", out),
    }
}

fn test(
    o: &Obj<'_>,
    lang: Lang,
    top: bool,
    depth: usize,
    out: &mut Vec<Diagnostic>,
) -> Option<Test> {
    if depth > 32 {
        return None;
    }
    let mut extra = vec!["Comment"];
    if top {
        extra.extend(["Next", "Assign", "Output"]);
    }
    if lang == Lang::Jsonata {
        let mut allowed = vec!["Condition"];
        allowed.extend(extra);
        o.only(&allowed, out);
        o.require(&["Condition"], out);
        return o.get("Condition").cloned().map(Test::Condition);
    }
    if let Some(list) = o.get("And").or_else(|| o.get("Or")) {
        let and = o.get("And").is_some();
        let mut allowed = vec![if and { "And" } else { "Or" }];
        allowed.extend(&extra);
        o.only(&allowed, out);
        let key = if and { "And" } else { "Or" };
        let mut all = Vec::new();
        for (i, t) in list.as_array().into_iter().flatten().enumerate() {
            if let Value::Object(m) = t {
                let sub = Obj::new(m, format!("{}/{key}/{i}", o.at));
                all.extend(test(&sub, lang, false, depth + 1, out));
            }
        }
        return Some(if and { Test::And(all) } else { Test::Or(all) });
    }
    if let Some(Value::Object(m)) = o.get("Not") {
        let mut allowed = vec!["Not"];
        allowed.extend(&extra);
        o.only(&allowed, out);
        let sub = Obj::new(m, format!("{}/Not", o.at));
        return test(&sub, lang, false, depth + 1, out).map(|t| Test::Not(Box::new(t)));
    }
    compare(o, &extra, out)
}

fn compare(o: &Obj<'_>, extra: &[&str], out: &mut Vec<Diagnostic>) -> Option<Test> {
    let found: Vec<(&String, CmpOp, bool)> = o
        .map
        .keys()
        .filter_map(|k| operator(k).map(|(op, path)| (k, op, path)))
        .collect();
    let mut allowed = vec!["Variable"];
    allowed.extend(extra);
    allowed.extend(found.iter().map(|(k, ..)| k.as_str()));
    o.only(&allowed, out);
    let Some((member, op, path)) = found.first().copied() else {
        // (memory)
        out.push(Diagnostic::error(
            SCHEMA,
            "A choice rule must have a comparison operator, or And, Or or Not",
            &o.at,
        ));
        return None;
    };
    o.require(&["Variable"], out);
    let variable = o.text("Variable", out).unwrap_or_default();
    let value = o.get(member).cloned().unwrap_or(Value::Null);
    let operand = if path {
        Operand::Path(value.as_str().unwrap_or_default().to_owned())
    } else {
        check_operand(op, &value, &o.here(member), out);
        Operand::Value(value)
    };
    Some(Test::Compare {
        variable,
        op,
        operand,
    })
}

fn check_operand(op: CmpOp, v: &Value, at: &str, out: &mut Vec<Diagnostic>) {
    let ok = match op.kind {
        CmpKind::String | CmpKind::Timestamp => v.is_string(),
        CmpKind::Numeric => v.is_number(),
        CmpKind::Boolean | CmpKind::Is => v.is_boolean(),
    };
    if !ok {
        let want = match op.kind {
            CmpKind::String | CmpKind::Timestamp => "a STRING",
            CmpKind::Numeric => "a NUMBER",
            _ => "a BOOLEAN",
        };
        let name = at.rsplit('/').next().unwrap_or_default();
        // (memory)
        out.push(Diagnostic::error(
            SCHEMA,
            format!("The value for the field '{name}' must be {want}"),
            at,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_read() {
        assert_eq!(
            operator("NumericGreaterThanEqualsPath"),
            Some((
                CmpOp {
                    kind: CmpKind::Numeric,
                    relation: "GreaterThanEquals"
                },
                true
            ))
        );
        assert_eq!(
            operator("IsPresent").map(|(o, _)| o.kind),
            Some(CmpKind::Is)
        );
        assert_eq!(
            operator("StringMatches").map(|(o, _)| o.relation),
            Some("Matches")
        );
        assert!(operator("BooleanLessThan").is_none());
        assert!(operator("Variable").is_none());
    }
}
