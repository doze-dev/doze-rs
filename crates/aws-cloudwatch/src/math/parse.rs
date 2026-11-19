//! A metric math expression read into a tree. Precedence, loosest first: `OR`, `AND`, `NOT`, the comparisons, `+ -`, `* /`, a
//! leading `-`, and `^` (which binds to the right). Functions are upper-case names followed by brackets; a list is `[a, b]`; an id
//! names another query of the request. Fuzzed by `fuzz_metric_math`.

// fuzz:parser fuzz_metric_math

use super::lex::{Op, Tok, lex};

/// How deep brackets and operators may nest: deeper is refused, so no input can exhaust the stack.
const MAX_DEPTH: usize = 64;

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    Num(f64),
    Str(String),
    /// A name that is not a function call: the id of another query, or a word such as `REPEAT` or `DESC`.
    Name(String),
    List(Vec<Expr>),
    Call(String, Vec<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Every name the expression refers to, outside function names.
    pub(crate) fn names(&self, out: &mut Vec<String>) {
        match self {
            Self::Name(n) => out.push(n.clone()),
            Self::List(items) | Self::Call(_, items) => {
                for i in items {
                    i.names(out);
                }
            }
            Self::Neg(e) | Self::Not(e) => e.names(out),
            Self::Bin(_, a, b) => {
                a.names(out);
                b.names(out);
            }
            Self::Num(_) | Self::Str(_) => {}
        }
    }

    /// Whether the expression calls `function` anywhere.
    pub(crate) fn calls(&self, function: &str) -> bool {
        match self {
            Self::Call(f, items) => f == function || items.iter().any(|i| i.calls(function)),
            Self::List(items) => items.iter().any(|i| i.calls(function)),
            Self::Neg(e) | Self::Not(e) => e.calls(function),
            Self::Bin(_, a, b) => a.calls(function) || b.calls(function),
            Self::Num(_) | Self::Str(_) | Self::Name(_) => false,
        }
    }
}

/// Reads `text` as an expression.
///
/// # Errors
/// What is wrong, in a sentence.
pub(crate) fn parse(text: &str) -> Result<Expr, String> {
    let toks = lex(text)?;
    if toks.is_empty() {
        return Err("the expression is empty".to_owned());
    }
    let mut p = Parser { toks, at: 0 };
    let e = p.expr(0, 0)?;
    match p.peek() {
        None => Ok(e),
        Some(t) => Err(format!("unexpected {} after the expression", shown(t))),
    }
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

/// How tightly an infix operator binds (left, right).
fn binding(op: Op) -> Option<(u8, u8)> {
    Some(match op {
        Op::Or => (1, 2),
        Op::And => (3, 4),
        Op::Lt | Op::Gt | Op::Le | Op::Ge | Op::Eq | Op::Ne => (7, 8),
        Op::Add | Op::Sub => (9, 10),
        Op::Mul | Op::Div => (11, 12),
        Op::Pow => (16, 15),
        Op::Not => return None,
    })
}

/// The binding of `NOT` and of a leading `-`.
const NOT_BINDS: u8 = 5;
const NEG_BINDS: u8 = 13;

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn expr(&mut self, min: u8, depth: usize) -> Result<Expr, String> {
        if depth > MAX_DEPTH {
            return Err("the expression nests too deeply".to_owned());
        }
        let mut left = self.prefix(depth)?;
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            let Some((l, r)) = binding(op) else {
                return Err("unexpected NOT inside an expression".to_owned());
            };
            if l < min {
                break;
            }
            self.at += 1;
            let right = self.expr(r, depth + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn prefix(&mut self, depth: usize) -> Result<Expr, String> {
        match self.next() {
            None => Err("the expression ends too early".to_owned()),
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Op(Op::Sub)) => Ok(Expr::Neg(Box::new(self.expr(NEG_BINDS, depth + 1)?))),
            Some(Tok::Op(Op::Not)) => Ok(Expr::Not(Box::new(self.expr(NOT_BINDS, depth + 1)?))),
            Some(Tok::Open) => {
                let e = self.expr(0, depth + 1)?;
                self.expect(&Tok::Close, "')'")?;
                Ok(e)
            }
            Some(Tok::OpenList) => Ok(Expr::List(self.items(&Tok::CloseList, depth)?)),
            Some(Tok::Name(n)) => {
                if self.peek() == Some(&Tok::Open) {
                    self.at += 1;
                    Ok(Expr::Call(n, self.items(&Tok::Close, depth)?))
                } else {
                    Ok(Expr::Name(n))
                }
            }
            Some(t) => Err(format!("unexpected {}", shown(&t))),
        }
    }

    /// Comma-separated expressions up to `close` (which is taken).
    fn items(&mut self, close: &Tok, depth: usize) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        if self.peek() == Some(close) {
            self.at += 1;
            return Ok(out);
        }
        loop {
            out.push(self.expr(0, depth + 1)?);
            match self.next() {
                Some(Tok::Comma) => {}
                Some(t) if &t == close => return Ok(out),
                Some(t) => return Err(format!("unexpected {}", shown(&t))),
                None => return Err("a bracket is not closed".to_owned()),
            }
        }
    }

    fn expect(&mut self, want: &Tok, what: &str) -> Result<(), String> {
        match self.next() {
            Some(t) if &t == want => Ok(()),
            Some(t) => Err(format!("expected {what}, found {}", shown(&t))),
            None => Err(format!("expected {what}")),
        }
    }
}

fn shown(t: &Tok) -> String {
    match t {
        Tok::Num(n) => format!("{n}"),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Name(n) => n.clone(),
        Tok::Open => "'('".to_owned(),
        Tok::Close => "')'".to_owned(),
        Tok::OpenList => "'['".to_owned(),
        Tok::CloseList => "']'".to_owned(),
        Tok::Comma => "','".to_owned(),
        Tok::Op(op) => format!("{op:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin(op: Op, a: Expr, b: Expr) -> Expr {
        Expr::Bin(op, Box::new(a), Box::new(b))
    }
    fn n(x: &str) -> Expr {
        Expr::Name(x.to_owned())
    }

    #[test]
    fn precedence_is_the_usual_one() {
        assert_eq!(
            parse("a + b * c").unwrap(),
            bin(Op::Add, n("a"), bin(Op::Mul, n("b"), n("c")))
        );
        assert_eq!(
            parse("2 ^ 3 ^ 2").unwrap(),
            bin(
                Op::Pow,
                Expr::Num(2.0),
                bin(Op::Pow, Expr::Num(3.0), Expr::Num(2.0))
            )
        );
        assert_eq!(
            parse("-a ^ 2").unwrap(),
            Expr::Neg(Box::new(bin(Op::Pow, n("a"), Expr::Num(2.0))))
        );
        assert_eq!(
            parse("a > 1 AND NOT b < 2 OR c").unwrap(),
            bin(
                Op::Or,
                bin(
                    Op::And,
                    bin(Op::Gt, n("a"), Expr::Num(1.0)),
                    Expr::Not(Box::new(bin(Op::Lt, n("b"), Expr::Num(2.0))))
                ),
                n("c")
            )
        );
    }

    #[test]
    fn calls_and_lists_are_read() {
        let e = parse("SUM([m1, m2]) / FILL(m3, REPEAT)").unwrap();
        let mut names = Vec::new();
        e.names(&mut names);
        assert_eq!(names, ["m1", "m2", "m3", "REPEAT"]);
        assert!(e.calls("FILL") && !e.calls("RATE"));
        assert_eq!(
            parse("METRICS()").unwrap(),
            Expr::Call("METRICS".into(), vec![])
        );
    }

    #[test]
    fn what_is_not_an_expression_is_refused() {
        for bad in ["", "a +", "(a", "SUM(a", "a b", "[a,", ")", "a NOT b"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse(&"(".repeat(200)).is_err());
        assert!(parse(&"-".repeat(500)).is_err());
    }
}
