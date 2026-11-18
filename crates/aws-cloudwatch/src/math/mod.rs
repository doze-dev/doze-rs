//! Metric math: the expressions of `GetMetricData` queries and of a metric math alarm's `Metrics`, read by a hand-written parser
//! (`parse`, `lex`, and `search` for `SEARCH`) and worked out over the queries' data (`eval`, `funcs`, `series_fns`). The model's
//! `MetricDataQuery` documentation is the spec for the queries (an id, a `MetricStat` or an `Expression`, `ReturnData`); the
//! functions and the syntax are AWS's user guide page as remembered (memory). A Metrics Insights query (`SELECT ...`) is refused with
//! a sentence: it is another language.

mod eval;
mod funcs;
mod lex;
mod parse;
mod search;
mod series_fns;
mod value;

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::req::dimensions;
pub(crate) use eval::is_id;
pub(crate) use funcs::MAX_SEARCH;
pub(crate) use parse::Expr;
pub(crate) use search::{Query as Search, matches};
pub(crate) use value::{Ts, Val};

/// A metric one query reads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stat {
    pub(crate) namespace: String,
    pub(crate) metric: String,
    pub(crate) dims: Vec<(String, String)>,
    pub(crate) stat: String,
    pub(crate) unit: Option<String>,
    pub(crate) period_s: i64,
}

/// What a query is.
#[derive(Clone, Debug)]
pub(crate) enum Body {
    Stat(Stat),
    Expr(Expr),
}

/// One `MetricDataQuery`.
#[derive(Clone, Debug)]
pub(crate) struct Query {
    pub(crate) id: String,
    pub(crate) label: Option<String>,
    pub(crate) return_data: bool,
    pub(crate) period_s: Option<i64>,
    pub(crate) body: Body,
}

/// Reads the queries of a request. `Err` is a sentence for a `ValidationError` (memory).
pub(crate) fn read(queries: &[Value]) -> Result<Vec<Query>, String> {
    let mut out: Vec<Query> = Vec::with_capacity(queries.len());
    for q in queries {
        let text = |k: &str| q.get(k).and_then(Value::as_str);
        let id = text("Id").unwrap_or_default().to_owned();
        if out.iter().any(|o| o.id == id) {
            return Err(format!(
                "The value {id} for field Id is used more than once."
            ));
        }
        let body = match (q.get("MetricStat"), text("Expression")) {
            (Some(s), None) => Body::Stat(stat_of(s)),
            (None, Some(e)) => Body::Expr(expression(&id, e)?),
            _ => {
                return Err(format!(
                    "The query {id} must have either a MetricStat or an Expression, and not both."
                ));
            }
        };
        out.push(Query {
            id,
            label: text("Label").map(str::to_owned),
            return_data: q.get("ReturnData").and_then(Value::as_bool).unwrap_or(true),
            period_s: q.get("Period").and_then(Value::as_i64),
            body,
        });
    }
    Ok(out)
}

fn expression(id: &str, text: &str) -> Result<Expr, String> {
    if text
        .trim_start()
        .get(..6)
        .is_some_and(|w| w.eq_ignore_ascii_case("select"))
    {
        return Err(format!(
            "Error in expression {id}: Metrics Insights queries (SELECT) are not supported in doze-aws; use metric math or SEARCH."
        ));
    }
    parse::parse(text).map_err(|e| format!("Error in expression {id}: {e}"))
}

fn stat_of(s: &Value) -> Stat {
    let metric = s.get("Metric").unwrap_or(&Value::Null);
    let text = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Stat {
        namespace: text(metric, "Namespace"),
        metric: text(metric, "MetricName"),
        dims: dimensions(metric.get("Dimensions")),
        stat: text(s, "Stat"),
        unit: s.get("Unit").and_then(Value::as_str).map(str::to_owned),
        period_s: s.get("Period").and_then(Value::as_i64).unwrap_or(60),
    }
}

/// The expressions in the order they can be worked out: each after the ids it names. An id no query has, and a query that comes
/// back to itself, are refused (memory).
pub(crate) fn order(queries: &[Query]) -> Result<Vec<usize>, String> {
    let index: HashMap<&str, usize> = queries
        .iter()
        .enumerate()
        .map(|(i, q)| (q.id.as_str(), i))
        .collect();
    let mut state = vec![0u8; queries.len()];
    let mut out = Vec::new();
    for i in 0..queries.len() {
        visit(i, queries, &index, &mut state, &mut out)?;
    }
    Ok(out)
}

fn visit(
    i: usize,
    queries: &[Query],
    index: &HashMap<&str, usize>,
    state: &mut [u8],
    out: &mut Vec<usize>,
) -> Result<(), String> {
    let (Some(q), Some(s)) = (queries.get(i), state.get(i).copied()) else {
        return Ok(());
    };
    match s {
        2 => return Ok(()),
        1 => return Err(format!("Error in expression {}: Circular dependency", q.id)),
        _ => {}
    }
    let Body::Expr(e) = &q.body else {
        if let Some(s) = state.get_mut(i) {
            *s = 2;
        }
        return Ok(());
    };
    if let Some(s) = state.get_mut(i) {
        *s = 1;
    }
    let mut names = Vec::new();
    e.names(&mut names);
    for n in names.iter().filter(|n| is_id(n)) {
        let j = *index
            .get(n.as_str())
            .ok_or_else(|| format!("Error in expression {}: Unrecognized identifier {n}", q.id))?;
        visit(j, queries, index, state, out)?;
    }
    if let Some(s) = state.get_mut(i) {
        *s = 2;
    }
    out.push(i);
    Ok(())
}

/// Where the data of a metric and of a search comes from: the samples on disk for `GetMetricData`, the data an alarm holds in
/// memory while it is judged.
pub(crate) trait Data {
    fn read(&self, stat: &Stat) -> Result<Ts, String>;
    fn search(&self, query: &Search, stat: &str, period_s: i64) -> Result<Vec<Ts>, String>;
}

struct Vars<'a> {
    values: &'a HashMap<String, Val>,
    metrics: &'a [(String, Ts)],
    data: &'a dyn Data,
}

impl eval::Source for Vars<'_> {
    fn var(&self, id: &str) -> Option<Val> {
        self.values.get(id).cloned()
    }

    fn metrics(&self) -> Vec<(String, Ts)> {
        self.metrics.to_vec()
    }

    fn search(&self, query: &Search, stat: &str, period_s: i64) -> Result<Vec<Ts>, String> {
        self.data.search(query, stat, period_s)
    }
}

/// Works out every query over the window whose periods start at `grid`: the metrics read, then the expressions in order.
pub(crate) fn solve(
    queries: &[Query],
    grid: &[i64],
    period_ms: i64,
    data: &dyn Data,
) -> Result<HashMap<String, Val>, String> {
    let mut values: HashMap<String, Val> = HashMap::new();
    let mut metrics = Vec::new();
    for q in queries {
        if let Body::Stat(s) = &q.body {
            let mut ts = data.read(s)?;
            ts.label = q.label.clone().unwrap_or_else(|| s.metric.clone());
            metrics.push((q.id.clone(), ts.clone()));
            values.insert(q.id.clone(), Val::Series(ts));
        }
    }
    for i in order(queries)? {
        let Some(Query {
            id,
            body: Body::Expr(e),
            ..
        }) = queries.get(i)
        else {
            continue;
        };
        let vars = Vars {
            values: &values,
            metrics: &metrics,
            data,
        };
        let env = eval::Env {
            grid,
            period_ms,
            src: &vars,
        };
        let v = eval::eval(e, &env).map_err(|m| format!("Error in expression {id}: {m}"))?;
        values.insert(id.clone(), v);
    }
    Ok(values)
}

/// The start of every period of `period_ms` from `start` up to `end`, at most `max`.
pub(crate) fn grid(start: i64, end: i64, period_ms: i64, max: usize) -> Vec<i64> {
    let step = period_ms.max(1);
    (0..)
        .map(|k| start + k * step)
        .take_while(|t| *t < end)
        .take(max)
        .collect()
}

/// A scalar shown as a series: its value at every timestamp `at` holds.
pub(crate) fn broadcast(x: f64, at: &[i64]) -> Ts {
    Ts::new("", at.iter().map(|t| (*t, x)).collect::<BTreeMap<_, _>>())
}

/// Whether `text` reads as a metric math expression (and, for each `SEARCH` in it, a search expression). Exists so the parser can
/// be fuzzed.
pub(crate) fn check(text: &str) -> bool {
    let math = parse::parse(text).is_ok();
    let searched = search::parse(text).is_ok();
    math || searched
}
