//! Choice states: the first rule that holds names the next state, then `Default`, then `States.NoChoiceMatched`. A comparison of a
//! value of the wrong type does not hold; a `Variable` that selects nothing fails the execution with `States.Runtime` (`IsPresent`
//! is the one safe test), as AWS does.

use serde_json::Value;

use super::io::View;
use super::run::{Move, no_match};
use crate::def::{CmpKind, CmpOp, Lang, Operand, Rule, Test};
use crate::errors::{Failure, RUNTIME};
use crate::path::{Path, Scope};

pub(crate) fn choose(v: &View<'_>, raw: &Value, rules: &[Rule], default: Option<&str>) -> Move {
    let attempt = || -> Result<Move, Failure> {
        let eff = if v.state.lang == Lang::Jsonata {
            raw.clone()
        } else {
            v.effective(raw)?
        };
        let mut chosen = None;
        for r in rules {
            if holds(v, &r.test, &eff, raw)? {
                chosen = Some(r);
                break;
            }
        }
        let next = match (chosen, default) {
            (Some(r), _) => r.next.clone(),
            (None, Some(d)) => Some(d.to_owned()),
            (None, None) => return Err(no_match()),
        };
        if v.state.lang == Lang::Jsonata {
            let states = v.states(raw, &[]);
            let output = match chosen
                .and_then(|r| r.output.as_ref())
                .or(v.state.output.as_ref())
            {
                Some(o) => v.jsonata("Output", o, &states)?.unwrap_or(Value::Null),
                None => raw.clone(),
            };
            let mut assigned = v.assign(v.state.assign.as_ref(), raw, &states)?;
            if let Some(more) = v.assign(chosen.and_then(|r| r.assign.as_ref()), raw, &states)? {
                assigned.get_or_insert_with(Default::default).extend(more);
            }
            return Ok(Move::Exit {
                output,
                next,
                assigned,
            });
        }
        let output = v.output(&eff)?;
        let mut assigned = v.assign(v.state.assign.as_ref(), &eff, &Value::Null)?;
        if let Some(more) = v.assign(chosen.and_then(|r| r.assign.as_ref()), &eff, &Value::Null)? {
            assigned.get_or_insert_with(Default::default).extend(more);
        }
        Ok(Move::Exit {
            output,
            next,
            assigned,
        })
    };
    attempt().unwrap_or_else(Move::Fail)
}

fn holds(v: &View<'_>, t: &Test, eff: &Value, raw: &Value) -> Result<bool, Failure> {
    match t {
        Test::And(all) => {
            for t in all {
                if !holds(v, t, eff, raw)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Test::Or(all) => {
            for t in all {
                if holds(v, t, eff, raw)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Test::Not(t) => Ok(!holds(v, t, eff, raw)?),
        Test::Condition(c) => {
            let got = v.jsonata("Condition", c, &v.states(raw, &[]))?;
            got.and_then(|g| g.as_bool()).ok_or_else(|| {
                // (memory)
                v.fail(
                    crate::errors::QUERY_EVALUATION_ERROR,
                    "The Condition must evaluate to a boolean",
                )
            })
        }
        Test::Compare {
            variable,
            op,
            operand,
        } => compare(v, eff, variable, *op, operand),
    }
}

fn at(v: &View<'_>, eff: &Value, p: &str) -> Option<Value> {
    let (ctx, vars) = (v.context(None), v.vars());
    Path::parse(p).ok()?.select(Scope {
        data: eff,
        context: &ctx,
        vars: &vars,
    })
}

fn compare(
    v: &View<'_>,
    eff: &Value,
    variable: &str,
    op: CmpOp,
    operand: &Operand,
) -> Result<bool, Failure> {
    let value = at(v, eff, variable);
    if op.kind == CmpKind::Is {
        let want = operand_value(v, eff, operand)?.as_bool().unwrap_or(true);
        return Ok(is(op.relation, value.as_ref()) == want);
    }
    let Some(value) = value else {
        // (memory)
        return Err(v.fail(
            RUNTIME,
            &format!("Invalid path '{variable}': The choice state's condition path references an invalid value."),
        ));
    };
    let other = operand_value(v, eff, operand)?;
    Ok(relate(op, &value, &other))
}

fn operand_value(v: &View<'_>, eff: &Value, o: &Operand) -> Result<Value, Failure> {
    match o {
        Operand::Value(x) => Ok(x.clone()),
        Operand::Path(p) => at(v, eff, p).ok_or_else(|| {
            v.fail(
                RUNTIME,
                &format!("Invalid path '{p}': The choice state's condition path references an invalid value."),
            )
        }),
    }
}

fn is(test: &str, value: Option<&Value>) -> bool {
    match (test, value) {
        ("Present", v) => v.is_some(),
        ("Null", Some(v)) => v.is_null(),
        ("Numeric", Some(v)) => v.is_number(),
        ("String", Some(v)) => v.is_string(),
        ("Boolean", Some(v)) => v.is_boolean(),
        ("Timestamp", Some(Value::String(s))) => crate::time::parse(s).is_some(),
        _ => false,
    }
}

fn relate(op: CmpOp, a: &Value, b: &Value) -> bool {
    use std::cmp::Ordering;
    let order: Option<Ordering> = match op.kind {
        CmpKind::String if op.relation == "Matches" => {
            return a
                .as_str()
                .zip(b.as_str())
                .is_some_and(|(t, p)| wildcard(t, p));
        }
        CmpKind::String => a.as_str().zip(b.as_str()).map(|(x, y)| x.cmp(y)),
        CmpKind::Numeric => a
            .as_f64()
            .zip(b.as_f64())
            .and_then(|(x, y)| x.partial_cmp(&y)),
        CmpKind::Boolean => return a.as_bool().is_some() && a.as_bool() == b.as_bool(),
        CmpKind::Timestamp => a
            .as_str()
            .and_then(crate::time::parse)
            .zip(b.as_str().and_then(crate::time::parse))
            .map(|(x, y)| x.cmp(&y)),
        CmpKind::Is => None,
    };
    order.is_some_and(|o| match op.relation {
        "Equals" => o.is_eq(),
        "LessThan" => o.is_lt(),
        "GreaterThan" => o.is_gt(),
        "LessThanEquals" => o.is_le(),
        _ => o.is_ge(),
    })
}

/// `StringMatches`: `*` is any run of characters, `\*` a star and `\\` a backslash.
fn wildcard(text: &str, pattern: &str) -> bool {
    let mut parts: Vec<String> = vec![String::new()];
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let (Some(e), Some(last)) = (chars.next(), parts.last_mut()) {
                    last.push(e);
                }
            }
            '*' => parts.push(String::new()),
            c => {
                if let Some(last) = parts.last_mut() {
                    last.push(c);
                }
            }
        }
    }
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
        return false;
    };
    if parts.len() == 1 {
        return text == first;
    }
    if !text.starts_with(first.as_str())
        || !text.ends_with(last.as_str())
        || text.len() < first.len() + last.len()
    {
        return false;
    }
    let mut rest = text
        .get(first.len()..text.len() - last.len())
        .unwrap_or_default();
    for mid in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        match rest.find(mid.as_str()) {
            Some(i) => rest = rest.get(i + mid.len()..).unwrap_or_default(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::wildcard;

    #[test]
    fn wildcards() {
        assert!(wildcard("log-2026.txt", "log-*.txt"));
        assert!(wildcard("abc", "*"));
        assert!(wildcard("a*c", r"a\*c"));
        assert!(!wildcard("abd", "a*c"));
        assert!(wildcard("abcbc", "a*bc"));
        assert!(!wildcard("ab", "ab*b"));
    }
}
