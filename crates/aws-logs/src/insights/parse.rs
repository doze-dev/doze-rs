//! The query parser: commands separated by `|`, each over expressions (precedence, lowest first: `or`, `and`, `not`, comparison,
//! `+ -`, `* / %`, `^`, unary minus).

use regex_lite::Regex;

use super::QueryError;
use super::ast::{Cmd, Expr, Named, Op, Parser, Period, Query};
use super::lex::{Lexer, Tok};
use super::value::Val;

/// The longest query AWS takes (the model's `QueryString` length).
const MAX_QUERY: usize = 10_000;
/// How deep expressions may nest, so a hostile query cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

/// Parses a query.
// fuzz:parser fuzz_insights_query
pub(crate) fn parse(src: &str) -> Result<Query, QueryError> {
    if src.len() > MAX_QUERY {
        return Err(QueryError::at(src, 0, "query is too long"));
    }
    let mut p = P {
        lx: Lexer::new(src),
        depth: 0,
    };
    let mut cmds = Vec::new();
    loop {
        if p.lx.peek()?.0 == Tok::End {
            break;
        }
        cmds.push(p.command()?);
        match p.lx.next()? {
            Tok::End => break,
            Tok::Sym("|") => {}
            _ => return Err(p.lx.error("unexpected symbol found")),
        }
    }
    Ok(Query { cmds })
}

pub(super) struct P<'a> {
    pub(super) lx: Lexer<'a>,
    depth: usize,
}

impl P<'_> {
    fn command(&mut self) -> Result<Cmd, QueryError> {
        let at = self.lx.pos;
        let word = match self.lx.next()? {
            Tok::Ident(w) => w.to_ascii_lowercase(),
            _ => return Err(QueryError::at(self.lx.src, at, "expected a command")),
        };
        Ok(match word.as_str() {
            "fields" => Cmd::Fields(self.named_list()?),
            "display" => Cmd::Display(self.named_list()?),
            "filter" => Cmd::Filter(self.expr()?),
            "stats" => Cmd::Stats(Box::new(self.stats()?)),
            "sort" => Cmd::Sort(self.sort_list()?),
            "limit" => Cmd::Limit(self.count()?),
            "parse" => self.parse_cmd()?,
            "dedup" => Cmd::Dedup(self.named_list()?.into_iter().map(|n| n.expr).collect()),
            "unmask" => Cmd::Unmask,
            "pattern" | "diff" | "anomaly" | "source" | "filterindex" | "unnest" | "lookup" => {
                return Err(QueryError::at(
                    self.lx.src,
                    at,
                    &format!("the {word} command is not run by doze-aws"),
                ));
            }
            _ => return Err(QueryError::at(self.lx.src, at, "unexpected symbol found")),
        })
    }

    fn count(&mut self) -> Result<usize, QueryError> {
        match self.lx.next()? {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Tok::Num(n) if n >= 1.0 && n.fract() == 0.0 => Ok(n as usize),
            _ => Err(self.lx.error("limit expects a positive whole number")),
        }
    }

    /// `expr [as name], ...`
    pub(super) fn named_list(&mut self) -> Result<Vec<Named>, QueryError> {
        let mut out = vec![self.named()?];
        while self.lx.eat(",")? {
            out.push(self.named()?);
        }
        Ok(out)
    }

    pub(super) fn named(&mut self) -> Result<Named, QueryError> {
        self.lx.skip();
        let start = self.lx.pos;
        let expr = self.expr()?;
        let text = self
            .lx
            .src
            .get(start..self.lx.pos)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let name = if self.lx.eat_word("as")? {
            self.name()?
        } else {
            match &expr {
                Expr::Field(f) => f.clone(),
                _ => text,
            }
        };
        Ok(Named { expr, name })
    }

    /// A field name.
    pub(super) fn name(&mut self) -> Result<String, QueryError> {
        match self.lx.next()? {
            Tok::Ident(w) | Tok::Quoted(w) => Ok(w),
            _ => Err(self.lx.error("expected a field name")),
        }
    }

    fn sort_list(&mut self) -> Result<Vec<(Expr, bool)>, QueryError> {
        let mut out = Vec::new();
        loop {
            let e = self.expr()?;
            let desc = if self.lx.eat_word("desc")? {
                true
            } else {
                self.lx.eat_word("asc")?;
                false
            };
            out.push((e, desc));
            if !self.lx.eat(",")? {
                return Ok(out);
            }
        }
    }

    fn parse_cmd(&mut self) -> Result<Cmd, QueryError> {
        let source = match self.lx.peek()? {
            (Tok::Str(_), _) => Expr::Field("@message".into()),
            _ if self.lx.peek_char() == Some('/') => Expr::Field("@message".into()),
            _ => Expr::Field(self.name()?),
        };
        if self.lx.peek_char() == Some('/') {
            let re = self.regex()?;
            return Ok(Cmd::Parse {
                source,
                how: Parser::Regex(Box::new(re)),
            });
        }
        let Tok::Str(glob) = self.lx.next()? else {
            return Err(self.lx.error("parse expects a pattern"));
        };
        let pieces: Vec<String> = glob.split('*').map(str::to_owned).collect();
        let mut names = Vec::new();
        if self.lx.eat_word("as")? {
            names.push(self.name()?);
            while self.lx.eat(",")? {
                names.push(self.name()?);
            }
        }
        if names.len() + 1 != pieces.len() {
            return Err(self.lx.error(
                "the number of fields in 'as' must match the number of wildcards in the pattern",
            ));
        }
        Ok(Cmd::Parse {
            source,
            how: Parser::Glob { pieces, names },
        })
    }

    fn regex(&mut self) -> Result<Regex, QueryError> {
        let at = self.lx.pos;
        let text = self.lx.regex()?;
        Regex::new(&text).map_err(|_| QueryError::at(self.lx.src, at, "invalid regular expression"))
    }

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
        let at = self.lx.pos;
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
            _ => return Err(QueryError::at(self.lx.src, at, "unexpected symbol found")),
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
            return super::funcs::checked(&lower, args)
                .map_err(|why| QueryError::at(self.lx.src, at, &why));
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
