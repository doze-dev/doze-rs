//! `stats`: its parsing (each aggregate becomes a slot) and the groups it gathers rows into.

use std::collections::{HashMap, HashSet};

use super::QueryError;
use super::ast::{Agg, AggKind, Expr, Named, Stats};
use super::eval::eval;
use super::parse::P;
use super::record::Row;
use super::value::{Val, compare};

/// The most values a percentile keeps for one group; past it the values are not kept (a local bound).
const MAX_PCT_VALUES: usize = 1_000_000;
/// The most distinct values `count_distinct` keeps for one group (a local bound; AWS answers an estimate).
const MAX_DISTINCT: usize = 1_000_000;

impl P<'_> {
    pub(super) fn stats(&mut self) -> Result<Stats, QueryError> {
        let at = self.lx.pos;
        let mut items = self.named_list()?;
        let by = if self.lx.eat_word("by")? {
            self.named_list()?
        } else {
            Vec::new()
        };
        let fail = |why: &str| QueryError::at(at, why);
        let mut aggs = Vec::new();
        for item in &mut items {
            let before = aggs.len();
            item.expr = lower(
                std::mem::replace(&mut item.expr, Expr::Star),
                &mut aggs,
                false,
            )
            .map_err(|w| fail(&w))?;
            if aggs.len() == before {
                return Err(fail("stats expects an aggregate function"));
            }
        }
        if by.iter().any(|b| has_agg(&b.expr)) {
            return Err(fail("an aggregate function cannot be used in 'by'"));
        }
        Ok(Stats { items, aggs, by })
    }
}

/// `e` with each aggregate replaced by its slot.
fn lower(e: Expr, aggs: &mut Vec<Agg>, inside: bool) -> Result<Expr, String> {
    Ok(match e {
        Expr::Agg(name, args) => {
            if inside || args.iter().any(has_agg) {
                return Err("aggregate functions cannot be nested".into());
            }
            aggs.push(spec(&name, args)?);
            Expr::Slot(aggs.len() - 1)
        }
        Expr::Neg(x) => Expr::Neg(Box::new(lower(*x, aggs, inside)?)),
        Expr::Not(x) => Expr::Not(Box::new(lower(*x, aggs, inside)?)),
        Expr::Bin(op, a, b) => Expr::Bin(
            op,
            Box::new(lower(*a, aggs, inside)?),
            Box::new(lower(*b, aggs, inside)?),
        ),
        Expr::Call(name, args) => Expr::Call(
            name,
            args.into_iter()
                .map(|a| lower(a, aggs, inside))
                .collect::<Result<_, _>>()?,
        ),
        other => other,
    })
}

fn has_agg(e: &Expr) -> bool {
    match e {
        Expr::Agg(..) => true,
        Expr::Neg(x) | Expr::Not(x) => has_agg(x),
        Expr::Bin(_, a, b) => has_agg(a) || has_agg(b),
        Expr::Call(_, args) | Expr::List(args) => args.iter().any(has_agg),
        _ => false,
    }
}

fn spec(name: &str, mut args: Vec<Expr>) -> Result<Agg, String> {
    let kind = AggKind::of(name).ok_or_else(|| format!("unknown function '{name}'"))?;
    let wrong = || format!("wrong number of arguments to '{name}'");
    let (arg, param) = match (kind, args.len()) {
        (AggKind::Count, 0) => (None, 0.0),
        (AggKind::Count, 1) if matches!(args.first(), Some(Expr::Star)) => (None, 0.0),
        (AggKind::Pct, 2) => {
            let p = match args.pop() {
                Some(Expr::Lit(v)) => v.num().filter(|p| (0.0..=100.0).contains(p)),
                _ => None,
            };
            (
                args.pop(),
                p.ok_or("pct expects a percentile from 0 to 100")?,
            )
        }
        (AggKind::Pct, _) => return Err(wrong()),
        (_, 1) if !matches!(args.first(), Some(Expr::Star)) => (args.pop(), 0.0),
        _ => return Err(wrong()),
    };
    Ok(Agg { kind, arg, param })
}

/// What one aggregate has gathered for one group.
#[derive(Debug)]
enum Acc {
    Count(u64),
    Distinct(HashSet<String>),
    Sum(Option<f64>),
    /// Count, mean and the sum of squared differences (Welford), for `avg` and `stddev`.
    Moments(u64, f64, f64),
    Pick(Option<Val>),
    Values(Vec<f64>),
    Timed(Option<(i64, Val)>),
}

impl Acc {
    fn new(kind: AggKind) -> Self {
        match kind {
            AggKind::Count => Self::Count(0),
            AggKind::CountDistinct => Self::Distinct(HashSet::new()),
            AggKind::Sum => Self::Sum(None),
            AggKind::Avg | AggKind::Stddev => Self::Moments(0, 0.0, 0.0),
            AggKind::Pct => Self::Values(Vec::new()),
            AggKind::Earliest | AggKind::Latest => Self::Timed(None),
            AggKind::Min | AggKind::Max | AggKind::SortsFirst | AggKind::SortsLast => {
                Self::Pick(None)
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn add(&mut self, a: &Agg, row: &Row) {
        let v = a
            .arg
            .as_ref()
            .map_or(Val::Bool(true), |e| eval(e, row, &[]));
        if v.is_null() {
            return;
        }
        match self {
            Self::Count(n) => *n += 1,
            Self::Distinct(set) if set.len() < MAX_DISTINCT => {
                set.insert(v.text());
            }
            Self::Distinct(_) => {}
            Self::Sum(s) => {
                if let Some(x) = v.num() {
                    *s = Some(s.unwrap_or(0.0) + x);
                }
            }
            Self::Moments(n, mean, m2) => {
                if let Some(x) = v.num() {
                    *n += 1;
                    let d = x - *mean;
                    *mean += d / *n as f64;
                    *m2 += d * (x - *mean);
                }
            }
            Self::Values(vs) if vs.len() < MAX_PCT_VALUES => vs.extend(v.num()),
            Self::Values(_) => {}
            Self::Pick(best) => {
                let wins = best.as_ref().is_none_or(|b| {
                    let ord = compare(&v, b);
                    match a.kind {
                        AggKind::Min | AggKind::SortsFirst => ord.is_lt(),
                        _ => ord.is_gt(),
                    }
                });
                if wins {
                    *best = Some(v);
                }
            }
            Self::Timed(best) => {
                let ts = row.ts().unwrap_or(0);
                let wins = best.as_ref().is_none_or(|(t, _)| match a.kind {
                    AggKind::Earliest => ts < *t,
                    _ => ts > *t,
                });
                if wins {
                    *best = Some((ts, v));
                }
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn finish(self, a: &Agg) -> Val {
        match self {
            Self::Count(n) => Val::Num(n as f64),
            Self::Distinct(set) => Val::Num(set.len() as f64),
            Self::Sum(s) => s.map_or(Val::Null, Val::Num),
            Self::Moments(0, ..) => Val::Null,
            Self::Moments(n, mean, m2) => match a.kind {
                AggKind::Stddev if n > 1 => Val::Num((m2 / (n - 1) as f64).sqrt()),
                AggKind::Stddev => Val::Num(0.0),
                _ => Val::Num(mean),
            },
            Self::Values(mut vs) => percentile(&mut vs, a.param),
            Self::Pick(v) => v.unwrap_or(Val::Null),
            Self::Timed(v) => v.map_or(Val::Null, |(_, v)| v),
        }
    }
}

/// The nearest-rank percentile. (memory: AWS's `pct` is an estimate; this one is exact up to the local bound.)
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn percentile(vs: &mut [f64], p: f64) -> Val {
    if vs.is_empty() {
        return Val::Null;
    }
    vs.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * vs.len() as f64).ceil().max(1.0) as usize;
    vs.get(rank.min(vs.len()) - 1)
        .copied()
        .map_or(Val::Null, Val::Num)
}

/// The groups of a stats command, in the order first seen.
#[derive(Debug, Default)]
pub(crate) struct Groups {
    index: HashMap<Vec<String>, usize>,
    groups: Vec<(Vec<Val>, Vec<Acc>)>,
}

impl Groups {
    pub(crate) fn len(&self) -> usize {
        self.groups.len()
    }

    pub(crate) fn add(&mut self, s: &Stats, row: &Row) {
        let keys: Vec<Val> = s.by.iter().map(|b| eval(&b.expr, row, &[])).collect();
        let texts: Vec<String> = keys.iter().map(Val::text).collect();
        let i = if let Some(i) = self.index.get(&texts) {
            *i
        } else {
            self.groups
                .push((keys, s.aggs.iter().map(|a| Acc::new(a.kind)).collect()));
            self.index.insert(texts, self.groups.len() - 1);
            self.groups.len() - 1
        };
        if let Some((_, accs)) = self.groups.get_mut(i) {
            for (acc, a) in accs.iter_mut().zip(&s.aggs) {
                acc.add(a, row);
            }
        }
    }

    /// A row per group: the `by` fields, then the items. Ordered by the `by` values. (memory)
    pub(crate) fn rows(self, s: &Stats) -> Vec<Row> {
        let mut groups = self.groups;
        groups.sort_by(|a, b| {
            a.0.iter()
                .zip(&b.0)
                .map(|(x, y)| compare(x, y))
                .find(|o| o.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        groups
            .into_iter()
            .map(|(keys, accs)| {
                let slots: Vec<Val> = accs
                    .into_iter()
                    .zip(&s.aggs)
                    .map(|(acc, a)| acc.finish(a))
                    .collect();
                let mut row = Row::default();
                for (b, k) in s.by.iter().zip(keys) {
                    row.set(&b.name, k);
                }
                let empty = Row::default();
                for Named { expr, name } in &s.items {
                    row.set(name, eval(expr, &empty, &slots));
                }
                row
            })
            .collect()
    }
}
