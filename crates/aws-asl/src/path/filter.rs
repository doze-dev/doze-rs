//! Filter expressions, `[?(@.price < 10 && @.tag)]`: comparisons of `@` (the node tested) or `$` paths with literals, `&&`, `||`, `!`
//! and parentheses, and Jayway's `=~ /regex/flags`, `in`, `nin`, `empty`. A path alone tests that the node has it.

use serde_json::Value;

use super::parse::{Cursor, steps};
use super::{Path, Root, Scope};

/// A parsed filter.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
    Not(Box<Filter>),
    Exists(Operand),
    Compare(Operand, Op, Operand),
}

/// One side of a comparison.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// `@...`: relative to the node tested.
    Here(Path),
    /// `$...`: from the document's root.
    Root(Path),
    Literal(Value),
    Regex(String, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Matches,
    In,
    Nin,
    Empty,
}

pub(super) fn parse(c: &mut Cursor<'_>) -> Result<Filter, String> {
    c.blank();
    if !c.eat("(") {
        return Err("a filter must be written ?(...)".to_owned());
    }
    let f = or(c)?;
    c.blank();
    if !c.eat(")") {
        return Err("a filter is not closed".to_owned());
    }
    Ok(f)
}

fn or(c: &mut Cursor<'_>) -> Result<Filter, String> {
    let mut left = and(c)?;
    loop {
        c.blank();
        if !c.eat("||") {
            return Ok(left);
        }
        left = Filter::Or(Box::new(left), Box::new(and(c)?));
    }
}

fn and(c: &mut Cursor<'_>) -> Result<Filter, String> {
    let mut left = unary(c)?;
    loop {
        c.blank();
        if !c.eat("&&") {
            return Ok(left);
        }
        left = Filter::And(Box::new(left), Box::new(unary(c)?));
    }
}

fn unary(c: &mut Cursor<'_>) -> Result<Filter, String> {
    c.blank();
    if c.eat("!") && !c.rest().starts_with('=') {
        return Ok(Filter::Not(Box::new(unary(c)?)));
    }
    if c.eat("(") {
        let f = or(c)?;
        c.blank();
        if !c.eat(")") {
            return Err("a ( is not closed".to_owned());
        }
        return Ok(f);
    }
    let left = operand(c)?;
    c.blank();
    let Some(op) = op(c) else {
        return Ok(Filter::Exists(left));
    };
    if op == Op::Empty {
        let want = operand(c)?;
        return Ok(Filter::Compare(left, op, want));
    }
    Ok(Filter::Compare(left, op, operand(c)?))
}

fn op(c: &mut Cursor<'_>) -> Option<Op> {
    let ops = [
        ("==", Op::Eq),
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("<", Op::Lt),
        (">", Op::Gt),
        ("=~", Op::Matches),
        ("nin ", Op::Nin),
        ("in ", Op::In),
        ("empty ", Op::Empty),
    ];
    ops.into_iter().find(|(s, _)| c.eat(s)).map(|(_, o)| o)
}

fn operand(c: &mut Cursor<'_>) -> Result<Operand, String> {
    c.blank();
    if c.eat("@") {
        return Ok(Operand::Here(relative(c, Root::Input)?));
    }
    if c.rest().starts_with('$') {
        c.eat("$");
        return Ok(Operand::Root(relative(c, Root::Input)?));
    }
    if matches!(c.peek(), Some('\'' | '"')) {
        return Ok(Operand::Literal(Value::String(c.quoted()?)));
    }
    if c.eat("/") {
        return regex(c);
    }
    literal(c)
}

fn relative(c: &mut Cursor<'_>, root: Root) -> Result<Path, String> {
    let start = c.at;
    let segs = steps(c, true)?;
    Ok(Path {
        root,
        segs,
        text: c.text.get(start..c.at).unwrap_or_default().to_owned(),
    })
}

fn regex(c: &mut Cursor<'_>) -> Result<Operand, String> {
    let rest = c.rest();
    let mut escaped = false;
    let end = rest
        .char_indices()
        .find(|(_, ch)| {
            let close = *ch == '/' && !escaped;
            escaped = *ch == '\\' && !escaped;
            close
        })
        .map(|(i, _)| i)
        .ok_or_else(|| "a regular expression is not closed".to_owned())?;
    let pattern = rest.get(..end).unwrap_or_default().to_owned();
    c.at += end + 1;
    let flags: String = c
        .rest()
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    c.at += flags.len();
    Ok(Operand::Regex(pattern, flags))
}

fn literal(c: &mut Cursor<'_>) -> Result<Operand, String> {
    let rest = c.rest();
    if rest.starts_with('[') {
        let mut depth = 0;
        let end = rest
            .char_indices()
            .find(|(_, ch)| {
                depth += match ch {
                    '[' => 1,
                    ']' => -1,
                    _ => 0,
                };
                depth == 0
            })
            .map_or(rest.len(), |(i, _)| i + 1);
        let text = rest.get(..end).unwrap_or_default().replace('\'', "\"");
        c.at += end;
        return serde_json::from_str(&text)
            .map(Operand::Literal)
            .map_err(|_| "a list in a filter is not valid".to_owned());
    }
    let word: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+'))
        .collect();
    c.at += word.len();
    serde_json::from_str::<Value>(&word)
        .map(Operand::Literal)
        .map_err(|_| format!("'{word}' is not a value a filter can compare"))
}

impl Filter {
    /// Whether `node` passes.
    #[must_use]
    pub fn holds(&self, node: &Value, scope: Scope<'_>) -> bool {
        match self {
            Self::And(a, b) => a.holds(node, scope) && b.holds(node, scope),
            Self::Or(a, b) => a.holds(node, scope) || b.holds(node, scope),
            Self::Not(f) => !f.holds(node, scope),
            Self::Exists(o) => o.value(node, scope).is_some(),
            Self::Compare(a, op, b) => compare(a.value(node, scope), *op, b, node, scope),
        }
    }
}

impl Operand {
    fn value(&self, node: &Value, scope: Scope<'_>) -> Option<Value> {
        match self {
            Self::Here(p) => p.select(Scope {
                data: node,
                ..scope
            }),
            Self::Root(p) => p.select(scope),
            Self::Literal(v) => Some(v.clone()),
            Self::Regex(..) => None,
        }
    }
}

fn compare(left: Option<Value>, op: Op, right: &Operand, node: &Value, scope: Scope<'_>) -> bool {
    let Some(left) = left else {
        return op == Op::Ne;
    };
    if let (Op::Matches, Operand::Regex(pattern, flags)) = (op, right) {
        let Some(text) = left.as_str() else {
            return false;
        };
        return regress::Regex::with_flags(pattern, flags.as_str())
            .is_ok_and(|r| r.find(text).is_some());
    }
    let Some(right) = right.value(node, scope) else {
        return op == Op::Ne;
    };
    match op {
        Op::Eq => same(&left, &right),
        Op::Ne => !same(&left, &right),
        Op::In => right
            .as_array()
            .is_some_and(|a| a.iter().any(|v| same(v, &left))),
        Op::Nin => !right
            .as_array()
            .is_some_and(|a| a.iter().any(|v| same(v, &left))),
        Op::Empty => {
            let empty = match &left {
                Value::Array(a) => a.is_empty(),
                Value::String(s) => s.is_empty(),
                Value::Object(o) => o.is_empty(),
                _ => false,
            };
            right.as_bool() == Some(empty)
        }
        Op::Lt | Op::Le | Op::Gt | Op::Ge => order(&left, &right).is_some_and(|o| match op {
            Op::Lt => o.is_lt(),
            Op::Le => o.is_le(),
            Op::Gt => o.is_gt(),
            _ => o.is_ge(),
        }),
        Op::Matches => false,
    }
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < f64::EPSILON,
        _ => a == b,
    }
}

fn order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64()?.partial_cmp(&y.as_f64()?),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}
