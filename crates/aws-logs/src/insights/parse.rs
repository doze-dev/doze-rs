//! The query parser: commands separated by `|`, each over expressions (precedence, lowest first: `or`, `and`, `not`, comparison,
//! `+ -`, `* / %`, `^`, unary minus).

use regex_lite::Regex;

use super::QueryError;
use super::ast::{Cmd, Expr, Named, Parser, Query};
use super::lex::{Lexer, Tok};

/// The longest query AWS takes (the model's `QueryString` length).
const MAX_QUERY: usize = 10_000;
/// How deep expressions may nest, so a hostile query cannot exhaust the stack.
pub(super) const MAX_DEPTH: usize = 64;

/// Parses a query.
// fuzz:parser fuzz_insights_query
pub(crate) fn parse(src: &str) -> Result<Query, QueryError> {
    if src.len() > MAX_QUERY {
        return Err(QueryError::at(0, "query is too long"));
    }
    let mut p = P {
        lx: Lexer::new(src),
        depth: 0,
    };
    let (mut cmds, mut spans) = (Vec::new(), Vec::new());
    loop {
        if p.lx.peek()?.0 == Tok::End {
            break;
        }
        let start = p.lx.pos;
        cmds.push(p.command()?);
        let text = src.get(start..p.lx.pos).unwrap_or_default();
        spans.push((start, start + text.trim_end().len()));
        if p.lx.peek()?.0 == Tok::Sym("|") {
            p.lx.next()?;
        } else if p.lx.peek()?.0 == Tok::End {
            break;
        } else {
            return Err(p.unexpected()?);
        }
    }
    ephemeral_once(&cmds, &spans)?;
    Ok(Query { cmds })
}

/// Why a query that names no log group is refused. AWS then reads it as one that must name its own with `SOURCE` (recorded
/// 2026-11-19, `StartQuery` and `CreateScheduledQuery`: "Invalid syntax while using query definition snippets: unexpected symbol
/// found fields at line 1 and position 0"); doze-aws does not run `SOURCE`, and says so for a query that begins with it.
pub(crate) fn without_groups(src: &str) -> QueryError {
    if src
        .trim_start()
        .get(..6)
        .is_some_and(|w| w.eq_ignore_ascii_case("source"))
    {
        return parse(src)
            .err()
            .unwrap_or_else(|| QueryError::at(0, "the source command is not run by doze-aws"));
    }
    let mut p = P {
        lx: Lexer::new(src),
        depth: 0,
    };
    p.unexpected()
        .unwrap_or_else(|e| e)
        .prefixed("Invalid syntax while using query definition snippets: ")
}

impl QueryError {
    fn prefixed(mut self, before: &str) -> Self {
        self.what = format!("{before}{}", self.what);
        self
    }
}

/// A field `parse` made may not be named again by a bare `fields` (recorded 2026-11-19: `parse ... as user, action | fields
/// user, action` is "Ephemeral field is already defined: user ([127,145])", the range that of the `fields` command).
fn ephemeral_once(cmds: &[Cmd], spans: &[(usize, usize)]) -> Result<(), QueryError> {
    let mut made: Vec<String> = Vec::new();
    for (c, (start, end)) in cmds.iter().zip(spans) {
        match c {
            Cmd::Parse {
                how: Parser::Glob { names, .. },
                ..
            } => made.extend(names.iter().cloned()),
            Cmd::Parse {
                how: Parser::Regex(re),
                ..
            } => {
                made.extend(re.capture_names().flatten().map(str::to_owned));
            }
            Cmd::Fields(list) => {
                for n in list {
                    if matches!(&n.expr, Expr::Field(f) if *f == n.name) && made.contains(&n.name) {
                        return Err(QueryError::span(
                            *start,
                            *end,
                            &format!("Ephemeral field is already defined: {}", n.name),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) struct P<'a> {
    pub(super) lx: Lexer<'a>,
    pub(super) depth: usize,
}

impl P<'_> {
    /// The token at the position refused, worded as AWS words it (recorded 2026-11-19: "unexpected symbol found | at line 1 and
    /// position 83"; the end of the query is (memory) ANTLR's `<EOF>`).
    pub(super) fn unexpected(&mut self) -> Result<QueryError, QueryError> {
        self.lx.skip();
        let at = self.lx.pos;
        let (tok, end) = self.lx.peek()?;
        let text = if tok == Tok::End {
            "<EOF>"
        } else {
            self.lx.src.get(at..end).unwrap_or_default()
        };
        Ok(QueryError::at(
            at,
            &format!("unexpected symbol found {text}"),
        ))
    }

    fn command(&mut self) -> Result<Cmd, QueryError> {
        self.lx.skip();
        let at = self.lx.pos;
        let word = match self.lx.peek()?.0 {
            Tok::Ident(w) => w.to_ascii_lowercase(),
            _ => return Err(self.unexpected()?),
        };
        let unknown = self.unexpected()?;
        self.lx.next()?;
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
                    at,
                    &format!("the {word} command is not run by doze-aws"),
                ));
            }
            _ => return Err(unknown),
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

    pub(super) fn regex(&mut self) -> Result<Regex, QueryError> {
        let at = self.lx.pos;
        let text = self.lx.regex()?;
        Regex::new(&text).map_err(|_| QueryError::at(at, "invalid regular expression"))
    }
}
