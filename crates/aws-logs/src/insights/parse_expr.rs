//! The expression half of the query parser: precedence climbing from `or` down to primaries, and the helpers it builds with.

use super::QueryError;
use super::ast::{Expr, Op, Period};
use super::lex::Tok;
use super::parse::{MAX_DEPTH, P};
use super::value::Val;

impl P<'_> {
    pub(super) fn expr(&mut self) -> Result<Expr, QueryError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.lx.error("expression is nested too deeply"));
        }
        let e = self.or();
        self.depth -= 1;
        e
    }

    fn or(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.and()?;
        while self.lx.eat_word("or")? {
            left = bin(Op::Or, left, self.and()?);
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.not()?;
        while self.lx.eat_word("and")? {
            left = bin(Op::And, left, self.not()?);
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, QueryError> {
        if self.lx.eat_word("not")? {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err(self.lx.error("expression is nested too deeply"));
            }
            let inner = self.not();
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(inner?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, QueryError> {
        let left = self.additive()?;
        let (tok, end) = self.lx.peek()?;
        let op = match tok {
            Tok::Sym("=") => Op::Eq,
            Tok::Sym("!=") => Op::Ne,
            Tok::Sym("<") => Op::Lt,
            Tok::Sym("<=") => Op::Le,
            Tok::Sym(">") => Op::Gt,
            Tok::Sym(">=") => Op::Ge,
            Tok::Sym("=~") => Op::Like,
            Tok::Ident(w) if w.eq_ignore_ascii_case("like") => Op::Like,
            Tok::Ident(w) if w.eq_ignore_ascii_case("in") => Op::In,
            Tok::Ident(w) if w.eq_ignore_ascii_case("not") => {
                let save = self.lx.pos;
                self.lx.pos = end;
                if self.lx.eat_word("like")? {
                    return Ok(bin(Op::NotLike, left, self.additive()?));
                }
                if self.lx.eat_word("in")? {
                    return Ok(bin(Op::NotIn, left, self.additive()?));
                }
                self.lx.pos = save;
                return Ok(left);
            }
            _ => return Ok(left),
        };
        self.lx.pos = end;
        Ok(bin(op, left, self.additive()?))
    }

    fn additive(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.multiplicative()?;
        loop {
            let op = if self.lx.eat("+")? {
                Op::Add
            } else if self.lx.eat("-")? {
                Op::Sub
            } else {
                return Ok(left);
            };
            left = bin(op, left, self.multiplicative()?);
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.power()?;
        loop {
            let op = if self.lx.eat("*")? {
                Op::Mul
            } else if self.lx.eat("/")? {
                Op::Div
            } else if self.lx.eat("%")? {
                Op::Mod
            } else {
                return Ok(left);
            };
            left = bin(op, left, self.power()?);
        }
    }

    fn power(&mut self) -> Result<Expr, QueryError> {
        let base = self.unary()?;
        if self.lx.eat("^")? {
            return Ok(bin(Op::Pow, base, self.unary()?));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<Expr, QueryError> {
        if self.lx.eat("-")? {
            return Ok(Expr::Neg(Box::new(self.primary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, QueryError> {
        if self.lx.peek_char() == Some('/') {
            return Ok(Expr::Regex(Box::new(self.regex()?)));
        }
        self.lx.skip();
        let at = self.lx.pos;
        let unexpected = self.unexpected()?;
        Ok(match self.lx.next()? {
            Tok::Num(n) => Expr::Lit(Val::Num(n)),
            Tok::Str(s) => Expr::Lit(Val::Str(s)),
            Tok::Period(n, unit) => Expr::Period(period(n, &unit)),
            Tok::Quoted(f) => Expr::Field(f),
            Tok::Sym("(") => {
                let e = self.expr()?;
                self.lx.expect(")")?;
                e
            }
            Tok::Sym("[") => Expr::List(self.items("]")?),
            Tok::Ident(w) => self.word(w, at)?,
            _ => return Err(unexpected),
        })
    }

    /// A name: `true`/`false`, a function call, or a field.
    fn word(&mut self, w: String, at: usize) -> Result<Expr, QueryError> {
        if self.lx.peek_char() == Some('(') {
            self.lx.expect("(")?;
            let lower = w.to_ascii_lowercase();
            let args = if self.lx.eat("*")? {
                self.lx.expect(")")?;
                vec![Expr::Star]
            } else {
                self.items(")")?
            };
            if !super::funcs::known(&lower) {
                // Recorded 2026-11-19: "Unrecognized function name: nosuch ([83,98])", the range the call's.
                return Err(QueryError::span(
                    at,
                    self.lx.pos,
                    &format!("Unrecognized function name: {w}"),
                ));
            }
            return super::funcs::checked(&lower, args).map_err(|why| QueryError::at(at, &why));
        }
        Ok(match w.to_ascii_lowercase().as_str() {
            "true" => Expr::Lit(Val::Bool(true)),
            "false" => Expr::Lit(Val::Bool(false)),
            _ => Expr::Field(w),
        })
    }

    /// Expressions separated by commas up to `close`.
    fn items(&mut self, close: &str) -> Result<Vec<Expr>, QueryError> {
        let mut out = Vec::new();
        if self.lx.eat(close)? {
            return Ok(out);
        }
        loop {
            out.push(self.expr()?);
            if self.lx.eat(close)? {
                return Ok(out);
            }
            self.lx.expect(",")?;
        }
    }
}

fn bin(op: Op, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

/// A period of `n` of `unit` (the lexer lets only the known units through), held to ten thousand years so the calendar arithmetic
/// cannot overflow.
#[allow(clippy::cast_possible_truncation)]
fn period(n: f64, unit: &str) -> Period {
    const MAX_MS: f64 = 10_000.0 * 366.0 * 86_400_000.0;
    let ms = |per: f64| Period::Ms((n * per).clamp(0.0, MAX_MS) as i64);
    let months = |per: f64| Period::Months((n * per).clamp(0.0, 120_000.0) as i64);
    match unit {
        "ms" | "msec" => ms(1.0),
        "s" | "sec" => ms(1000.0),
        "m" | "min" => ms(60_000.0),
        "h" | "hr" => ms(3_600_000.0),
        "d" => ms(86_400_000.0),
        "w" => ms(7.0 * 86_400_000.0),
        "mo" | "mon" => months(1.0),
        "q" => months(3.0),
        _ => months(12.0),
    }
}
