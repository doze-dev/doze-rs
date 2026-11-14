//! `count` and `for_each`: one block written, several declared (Go's `foreach.go`), and the old
//! `depends_on` map, read so that its error can say what to write instead.

use crate::error::{At, Result, Sources, diag_error, pos_err};
use crate::eval::eval;
use crate::model::Condition;
use doze_hcl::Value;
use doze_hcl::decode::Attr;
use doze_hcl::eval::Context;
use doze_hcl::value::quote::go_quote;
use std::collections::BTreeMap;

/// The most copies one `count` makes (a known difference from Go, which has no limit).
pub const MAX_COUNT: i64 = 10_000;

/// One declared copy: the name suffix ("" for a plain block) and its `each`/`count` variables.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stamp {
    pub suffix: String,
    pub vars: Option<BTreeMap<String, Value>>,
}

impl Stamp {
    pub(crate) fn name(&self, label: &str) -> String {
        if self.suffix.is_empty() {
            label.to_owned()
        } else {
            format!("{label}_{}", self.suffix)
        }
    }
}

fn at(a: &Attr<'_>) -> At {
    At::new(a.file, a.range)
}

/// The stamps of a block with `count`, `for_each`, or neither.
pub(crate) fn stamps(
    src: &dyn Sources,
    count: Option<&Attr<'_>>,
    for_each: Option<&Attr<'_>>,
    ctx: &Context<'_>,
) -> Result<Vec<Stamp>> {
    match (for_each, count) {
        (Some(a), _) => for_each_stamps(src, a, ctx),
        (None, Some(a)) => count_stamps(src, a, ctx),
        (None, None) => Ok(vec![Stamp::default()]),
    }
}

fn each(k: Value, v: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "each".to_owned(),
        Value::Object(BTreeMap::from([
            ("key".to_owned(), k),
            ("value".to_owned(), v),
        ])),
    )])
}

fn for_each_stamps(src: &dyn Sources, a: &Attr<'_>, ctx: &Context<'_>) -> Result<Vec<Stamp>> {
    let (v, ds) = eval(a, ctx);
    if ds.has_errors() {
        return Err(diag_error(src, ds));
    }
    let invalid = |d: &str| pos_err(src, &at(a), "invalid for_each", d);
    if v.is_null() || !v.is_known() {
        return Err(invalid("value must be a known map or set"));
    }
    let mut out = Vec::new();
    match &v {
        Value::Map(_, m) | Value::Object(m) => {
            for (k, e) in m {
                out.push(Stamp {
                    suffix: sanitize(k),
                    vars: Some(each(Value::string(k), e.clone())),
                });
            }
        }
        Value::Set(_, es) | Value::List(_, es) | Value::Tuple(es) => {
            for e in es {
                let Value::String(s) = e else {
                    return Err(invalid("set/list elements must be strings"));
                };
                out.push(Stamp {
                    suffix: sanitize(s),
                    vars: Some(each(e.clone(), e.clone())),
                });
            }
        }
        _ => return Err(invalid("value must be a map or a set of strings")),
    }
    out.sort_by(|x, y| x.suffix.cmp(&y.suffix));
    Ok(out)
}

fn count_stamps(src: &dyn Sources, a: &Attr<'_>, ctx: &Context<'_>) -> Result<Vec<Stamp>> {
    let (v, ds) = eval(a, ctx);
    if ds.has_errors() {
        return Err(diag_error(src, ds));
    }
    let Value::Number(n) = &v else {
        return Err(pos_err(
            src,
            &at(a),
            "invalid count",
            "value must be a non-negative whole number",
        ));
    };
    let n = match n.trunc() {
        Some((neg, mag)) => {
            let m = i64::try_from(mag).unwrap_or(i64::MAX);
            if neg { -m } else { m }
        }
        None => i64::MAX,
    };
    if n < 0 {
        return Err(pos_err(
            src,
            &at(a),
            "invalid count",
            "must not be negative",
        ));
    }
    // Go has no limit, and a typo of a few digits makes it build blocks until memory runs out.
    if n > MAX_COUNT {
        return Err(pos_err(
            src,
            &at(a),
            "invalid count",
            format!("must be at most {MAX_COUNT}"),
        ));
    }
    Ok((0..n)
        .map(|i| Stamp {
            suffix: i.to_string(),
            vars: Some(BTreeMap::from([(
                "count".to_owned(),
                Value::Object(BTreeMap::from([("index".to_owned(), Value::int(i))])),
            )])),
        })
        .collect())
}

/// Go's `parseDependsOn`: `{ "module.db" = "healthy" }`, by name.
pub(crate) fn parse_depends_on(
    src: &dyn Sources,
    a: &Attr<'_>,
    ctx: &Context<'_>,
) -> Result<BTreeMap<String, Condition>> {
    let (v, ds) = eval(a, ctx);
    if ds.has_errors() {
        return Err(diag_error(src, ds));
    }
    let mut out = BTreeMap::new();
    let m = match &v {
        Value::Null(_) => return Ok(out),
        Value::Object(m) | Value::Map(_, m) => m,
        _ => {
            return Err(pos_err(
                src,
                &at(a),
                "invalid depends_on",
                "expected a map of address to condition, e.g. { \"postgres.app\" = \"healthy\" }",
            ));
        }
    };
    for (k, e) in m {
        let Value::String(c) = e else {
            return Err(pos_err(
                src,
                &at(a),
                "invalid depends_on",
                "condition must be a string",
            ));
        };
        let cond = match c.as_str() {
            "healthy" => Condition::Healthy,
            "started" => Condition::Started,
            "lazy" => Condition::Lazy,
            _ => {
                return Err(pos_err(
                    src,
                    &at(a),
                    "invalid depends_on condition",
                    format!(
                        "{} is not a condition (want \"healthy\", \"started\" or \"lazy\")",
                        go_quote(c)
                    ),
                ));
            }
        };
        let name = k.split_once('.').map_or(k.as_str(), |(_, n)| n);
        out.insert(name.to_owned(), cond);
    }
    Ok(out)
}

/// A key as a name suffix: letters, digits, `_` and `-` kept, anything else `_`.
pub(crate) fn sanitize(key: &str) -> String {
    let s: String = key
        .chars()
        .map(|r| {
            if r.is_ascii_alphanumeric() || r == '_' || r == '-' {
                r
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "_".into() } else { s }
}
