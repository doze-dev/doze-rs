//! What a parsed query is: a pipeline of commands over expressions.

use regex_lite::Regex;

use super::value::Val;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Like,
    NotLike,
    In,
    NotIn,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// A length of time for `bin()`, `datefloor()` and `dateceil()`: fixed milliseconds, or whole calendar months.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Period {
    Ms(i64),
    Months(i64),
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Lit(Val),
    Field(String),
    Regex(Box<Regex>),
    List(Vec<Expr>),
    Period(Period),
    /// `*`, in `count(*)`.
    Star,
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    /// A function that is not an aggregate (its name lower-cased), and its arguments.
    Call(String, Vec<Expr>),
    /// An aggregate while a query is parsed (name, arguments); `stats` turns each into a [`Expr::Slot`].
    Agg(String, Vec<Expr>),
    /// The value of the stats command's aggregate number `n` for the row's group.
    Slot(usize),
}

/// An expression and the name its value is shown under: `as` gives one, otherwise the expression as written.
#[derive(Clone, Debug)]
pub(crate) struct Named {
    pub(crate) expr: Expr,
    pub(crate) name: String,
}

/// An aggregate of a stats command.
#[derive(Clone, Debug)]
pub(crate) struct Agg {
    pub(crate) kind: AggKind,
    /// What it reads; `None` for `count(*)` and `count()`.
    pub(crate) arg: Option<Expr>,
    /// The percentile of `pct`.
    pub(crate) param: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AggKind {
    Count,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
    Stddev,
    Pct,
    Earliest,
    Latest,
    SortsFirst,
    SortsLast,
}

impl AggKind {
    /// The aggregate of a function name (lower-cased), if it is one.
    pub(crate) fn of(name: &str) -> Option<Self> {
        Some(match name {
            "count" => Self::Count,
            "count_distinct" => Self::CountDistinct,
            "sum" => Self::Sum,
            "avg" => Self::Avg,
            "min" => Self::Min,
            "max" => Self::Max,
            "stddev" => Self::Stddev,
            "pct" => Self::Pct,
            "earliest" => Self::Earliest,
            "latest" => Self::Latest,
            "sortsfirst" => Self::SortsFirst,
            "sortslast" => Self::SortsLast,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Stats {
    /// What each row of the result shows, with [`Expr::Slot`] in place of each aggregate.
    pub(crate) items: Vec<Named>,
    pub(crate) aggs: Vec<Agg>,
    pub(crate) by: Vec<Named>,
}

/// How `parse` takes a field apart.
#[derive(Clone, Debug)]
pub(crate) enum Parser {
    /// Literal pieces with a `*` between each, each `*` a field named in `as`.
    Glob {
        pieces: Vec<String>,
        names: Vec<String>,
    },
    /// A regular expression whose named groups are the fields.
    Regex(Box<Regex>),
}

#[derive(Clone, Debug)]
pub(crate) enum Cmd {
    Fields(Vec<Named>),
    Display(Vec<Named>),
    Filter(Expr),
    Stats(Box<Stats>),
    Sort(Vec<(Expr, bool)>),
    Limit(usize),
    Parse {
        source: Expr,
        how: Parser,
    },
    Dedup(Vec<Expr>),
    /// `unmask`: nothing is masked locally.
    Unmask,
}

/// A parsed query.
#[derive(Clone, Debug)]
pub(crate) struct Query {
    pub(crate) cmds: Vec<Cmd>,
}
