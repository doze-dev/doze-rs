//! JSON patterns: `{ $.a.b = "x*" && ($.n >= 500 || $.list[0] IS NULL) }`.

use serde_json::Value;

use super::regex::Re;
use super::{PatternError, glob};

type R<T> = Result<T, PatternError>;

/// A path into a JSON document: `$.a.b[0]`, `$.items[*].id`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Selector(Vec<Step>);

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Key(String),
    Index(usize),
    All,
}

impl Selector {
    /// Reads `$`, `$.a`, `$.a[1].b`, `$.a[*]`, `$["a b"]`.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut rest = text.strip_prefix('$')?;
        let mut steps = Vec::new();
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('.') {
                let end = after.find(['.', '[']).unwrap_or(after.len());
                let (key, tail) = after.split_at(end);
                if key.is_empty() {
                    return None;
                }
                steps.push(Step::Key(key.to_owned()));
                rest = tail;
            } else {
                let after = rest.strip_prefix('[')?;
                let (inside, tail) = after.split_once(']')?;
                steps.push(match inside {
                    "*" => Step::All,
                    quoted
                        if quoted.starts_with('"') && quoted.ends_with('"') && quoted.len() > 1 =>
                    {
                        Step::Key(quoted.get(1..quoted.len() - 1)?.to_owned())
                    }
                    n => Step::Index(n.parse().ok()?),
                });
                rest = tail;
            }
        }
        Some(Self(steps))
    }

    /// Everything in `v` the path reaches.
    pub(crate) fn all<'v>(&self, v: &'v Value) -> Vec<&'v Value> {
        let mut found = vec![v];
        for step in &self.0 {
            found = found
                .into_iter()
                .flat_map(|x| match (step, x) {
                    (Step::Key(k), Value::Object(m)) => m.get(k).into_iter().collect(),
                    (Step::Index(i), Value::Array(a)) => a.get(*i).into_iter().collect(),
                    (Step::All, Value::Array(a)) => a.iter().collect(),
                    _ => Vec::new(),
                })
                .collect();
        }
        found
    }

    pub(crate) fn first<'v>(&self, v: &'v Value) -> Option<&'v Value> {
        self.all(v).into_iter().next()
    }
}

/// A value as text, for a metric value or a dimension.
pub(crate) fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[derive(Debug)]
pub(crate) enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Cmp(Selector, Op, Lit),
    Is(Selector, Lit),
    Exists(Selector, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Lit {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    /// `%...%`: the value (a string, or a number as written) contains a match.
    Re(Re),
}

impl Expr {
    pub(crate) fn matches(&self, v: &Value) -> bool {
        match self {
            Self::And(parts) => parts.iter().all(|p| p.matches(v)),
            Self::Or(parts) => parts.iter().any(|p| p.matches(v)),
            Self::Cmp(sel, op, lit) => sel.all(v).into_iter().any(|x| compare(x, *op, lit)),
            Self::Is(sel, lit) => sel.all(v).into_iter().any(|x| same(x, lit)),
            Self::Exists(sel, yes) => sel.first(v).is_some() == *yes,
        }
    }

    /// How many `%...%` the expression holds.
    pub(crate) fn regexes(&self) -> usize {
        match self {
            Self::And(parts) | Self::Or(parts) => parts.iter().map(Self::regexes).sum(),
            Self::Cmp(_, _, Lit::Re(_)) => 1,
            _ => 0,
        }
    }
}

/// Whether a value is equal to a literal (a string literal may hold `*`).
fn same(x: &Value, lit: &Lit) -> bool {
    match (x, lit) {
        (Value::String(s), Lit::Str(p)) => glob(p, s),
        (Value::String(s), Lit::Re(re)) => re.found(s),
        (Value::Number(n), Lit::Re(re)) => re.found(&n.to_string()),
        (Value::Number(n), Lit::Num(l)) => n.as_f64().is_some_and(|f| (f - l).abs() < f64::EPSILON),
        (Value::Bool(b), Lit::Bool(l)) => b == l,
        (Value::Null, Lit::Null) => true,
        _ => false,
    }
}

fn compare(x: &Value, op: Op, lit: &Lit) -> bool {
    match op {
        Op::Eq => same(x, lit),
        Op::Ne => !same(x, lit) && !x.is_null(),
        _ => match (x.as_f64(), lit) {
            (Some(a), Lit::Num(b)) => match op {
                Op::Lt => a < *b,
                Op::Gt => a > *b,
                Op::Le => a <= *b,
                _ => a >= *b,
            },
            _ => false,
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Open,
    Close,
    And,
    Or,
    Op(Op),
    Path(String),
    Lit(Lit),
    Word(String),
}

fn lex(text: &str) -> R<Vec<Tok>> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '(' => out.push(Tok::Open),
            ')' => out.push(Tok::Close),
            '&' | '|' => {
                if chars.next().map(|x| x.1) != Some(c) {
                    return Err(PatternError::Invalid);
                }
                out.push(if c == '&' { Tok::And } else { Tok::Or });
            }
            '=' => out.push(Tok::Op(Op::Eq)),
            '!' | '<' | '>' => {
                let eq = chars.peek().map(|x| x.1) == Some('=');
                if eq {
                    chars.next();
                }
                out.push(Tok::Op(match (c, eq) {
                    ('!', true) => Op::Ne,
                    ('<', true) => Op::Le,
                    ('<', false) => Op::Lt,
                    ('>', true) => Op::Ge,
                    ('>', false) => Op::Gt,
                    _ => return Err(PatternError::Invalid),
                }));
            }
            '"' => out.push(Tok::Lit(Lit::Str(quoted(&mut chars)?))),
            '%' => {
                let mut body = chars.by_ref().map(|x| x.1);
                out.push(Tok::Lit(Lit::Re(super::regex::read(&mut body)?)));
            }
            _ => {
                let end = text
                    .get(at..)
                    .and_then(|t| t.find(|x: char| x.is_whitespace() || "()=!<>&|".contains(x)))
                    .map_or(text.len(), |n| at + n);
                let word = text.get(at..end).ok_or(PatternError::Invalid)?;
                while chars.peek().is_some_and(|x| x.0 < end) {
                    chars.next();
                }
                out.push(classify(word));
            }
        }
    }
    Ok(out)
}

fn quoted(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> R<String> {
    let mut s = String::new();
    loop {
        match chars.next().map(|x| x.1) {
            Some('\\') => s.push(chars.next().ok_or(PatternError::Invalid)?.1),
            Some('"') => return Ok(s),
            Some(c) => s.push(c),
            None => return Err(PatternError::Invalid),
        }
    }
}

fn classify(word: &str) -> Tok {
    if word.starts_with('$') {
        return Tok::Path(word.to_owned());
    }
    if let Ok(n) = word.parse::<f64>() {
        return Tok::Lit(Lit::Num(n));
    }
    match word.to_ascii_uppercase().as_str() {
        "NULL" => Tok::Lit(Lit::Null),
        "TRUE" => Tok::Lit(Lit::Bool(true)),
        "FALSE" => Tok::Lit(Lit::Bool(false)),
        _ => Tok::Word(word.to_owned()),
    }
}

pub(super) fn parse(text: &str) -> R<Expr> {
    let inner = text
        .strip_prefix('{')
        .and_then(|t| t.trim_end().strip_suffix('}'))
        .ok_or(PatternError::Invalid)?;
    let toks = lex(inner)?;
    let mut p = Parser { toks, at: 0 };
    let e = p.or()?;
    if p.at == p.toks.len() {
        Ok(e)
    } else {
        Err(PatternError::Invalid)
    }
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn next(&mut self) -> R<Tok> {
        let t = self
            .toks
            .get(self.at)
            .cloned()
            .ok_or(PatternError::Invalid)?;
        self.at += 1;
        Ok(t)
    }

    fn eat(&mut self, want: &Tok) -> bool {
        let hit = self.peek() == Some(want);
        if hit {
            self.at += 1;
        }
        hit
    }

    fn or(&mut self) -> R<Expr> {
        let mut parts = vec![self.and()?];
        while self.eat(&Tok::Or) {
            parts.push(self.and()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::Or(parts)
        })
    }

    fn and(&mut self) -> R<Expr> {
        let mut parts = vec![self.atom()?];
        while self.eat(&Tok::And) {
            parts.push(self.atom()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::And(parts)
        })
    }

    fn atom(&mut self) -> R<Expr> {
        match self.next()? {
            Tok::Open => {
                let e = self.or()?;
                if self.eat(&Tok::Close) {
                    Ok(e)
                } else {
                    Err(PatternError::Invalid)
                }
            }
            Tok::Path(p) => {
                let sel = Selector::parse(&p).ok_or(PatternError::Invalid)?;
                self.after_path(sel)
            }
            _ => Err(PatternError::Invalid),
        }
    }

    fn after_path(&mut self, sel: Selector) -> R<Expr> {
        match self.next()? {
            Tok::Op(op) => match self.next()? {
                // An unquoted `*` tests that the field is there, whatever its type: `{ $.latency = * }` selects every event that has
                // one (a number, a string, a boolean, an object or null), and `!= *` every event that has none, as `NOT EXISTS`
                // does. The type-blind reading is from AWS's documentation and memory; no recording has a non-string field.
                Tok::Word(w) if w == "*" => match op {
                    Op::Eq => Ok(Expr::Exists(sel, true)),
                    Op::Ne => Ok(Expr::Exists(sel, false)),
                    _ => Err(PatternError::Invalid),
                },
                // An expression compares by `=` and `!=` alone.
                Tok::Lit(Lit::Re(_)) if !matches!(op, Op::Eq | Op::Ne) => {
                    Err(PatternError::Invalid)
                }
                Tok::Lit(lit) => Ok(Expr::Cmp(sel, op, lit)),
                Tok::Word(w) => Ok(Expr::Cmp(sel, op, Lit::Str(w))),
                _ => Err(PatternError::Invalid),
            },
            Tok::Word(w) if w.eq_ignore_ascii_case("IS") => match self.next()? {
                Tok::Lit(lit @ (Lit::Null | Lit::Bool(_))) => Ok(Expr::Is(sel, lit)),
                _ => Err(PatternError::Invalid),
            },
            Tok::Word(w) if w.eq_ignore_ascii_case("EXISTS") => Ok(Expr::Exists(sel, true)),
            Tok::Word(w) if w.eq_ignore_ascii_case("NOT") => match self.next()? {
                Tok::Word(x) if x.eq_ignore_ascii_case("EXISTS") => Ok(Expr::Exists(sel, false)),
                _ => Err(PatternError::Invalid),
            },
            _ => Err(PatternError::Invalid),
        }
    }
}
