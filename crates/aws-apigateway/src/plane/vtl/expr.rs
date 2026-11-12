//! Expressions inside directives and method arguments: literals (`'text'`, `"interpolated $x"`, numbers, `true`, `false`, `null`),
//! lists, ranges (`[1..3]`), maps (`{'k': v}`), references, and the operators by precedence: `||` `&&` `==` `!=` `<` `<=` `>` `>=`
//! `+` `-` `*` `/` `%` `!` (with the word forms `or`, `and`, `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `not`).

use serde_json::Value;

use super::parse::{Error, P, R, err};
use super::{Expr, Op};

impl P<'_> {
    pub(super) fn args(&mut self, close: char) -> R<Vec<Expr>> {
        let mut out = Vec::new();
        self.ws();
        if self.peek() == Some(close) {
            self.i += 1;
            return Ok(out);
        }
        loop {
            out.push(self.expr()?);
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some(c) if c == close => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return err("a list of arguments is not closed"),
            }
        }
    }

    pub(super) fn expr(&mut self) -> R<Expr> {
        self.deeper()?;
        let e = self.binary(0);
        self.depth -= 1;
        e
    }

    fn op_at(&mut self, level: usize) -> Option<Op> {
        self.ws();
        let table: &[(&str, Op)] = match level {
            0 => &[("||", Op::Or), ("or ", Op::Or)],
            1 => &[("&&", Op::And), ("and ", Op::And)],
            2 => &[
                ("==", Op::Eq),
                ("!=", Op::Ne),
                ("eq ", Op::Eq),
                ("ne ", Op::Ne),
            ],
            3 => &[
                ("<=", Op::Le),
                (">=", Op::Ge),
                ("<", Op::Lt),
                (">", Op::Gt),
                ("lt ", Op::Lt),
                ("le ", Op::Le),
                ("gt ", Op::Gt),
                ("ge ", Op::Ge),
            ],
            4 => &[("+", Op::Add), ("-", Op::Sub)],
            _ => &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Mod)],
        };
        table
            .iter()
            .find(|(t, _)| self.rest().starts_with(t))
            .map(|(t, op)| {
                self.i += t.len();
                *op
            })
    }

    fn binary(&mut self, level: usize) -> R<Expr> {
        if level > 5 {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = self.op_at(level) {
            let right = self.binary(level + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> R<Expr> {
        self.ws();
        if self.rest().starts_with("!") && !self.rest().starts_with("!=") {
            self.i += 1;
            self.deeper()?;
            let e = self.unary();
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(e?)));
        }
        if self.eat("not ") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.rest().starts_with('-')
            && self
                .rest()
                .get(1..2)
                .is_some_and(|c| !c.chars().all(|c| c.is_ascii_digit()))
        {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> R<Expr> {
        self.ws();
        let Some(c) = self.peek() else {
            return err("an expression ends early");
        };
        match c {
            '(' => {
                self.i += 1;
                let e = self.expr()?;
                self.ws();
                if !self.eat(")") {
                    return err("a ) is missing");
                }
                Ok(e)
            }
            '$' => match self.reference()? {
                Some((r, _)) => Ok(Expr::Ref(r)),
                None => err("a $ that is not a reference"),
            },
            '\'' => self.single(),
            '"' => self.double(),
            '[' => self.list(),
            '{' => self.map(),
            c if c.is_ascii_digit() || c == '-' => self.number(),
            _ => {
                for (word, v) in [
                    ("true", Value::Bool(true)),
                    ("false", Value::Bool(false)),
                    ("null", Value::Null),
                ] {
                    if self.eat(word) {
                        return Ok(Expr::Lit(v));
                    }
                }
                err(format!("unexpected {c:?} in an expression"))
            }
        }
    }

    fn single(&mut self) -> R<Expr> {
        self.i += 1;
        let Some(n) = self.rest().find('\'') else {
            return err("a string is not closed");
        };
        let s = self.rest().get(..n).unwrap_or_default().to_owned();
        self.i += n + 1;
        Ok(Expr::Lit(Value::String(s)))
    }

    fn double(&mut self) -> R<Expr> {
        self.i += 1;
        let Some(n) = self.rest().find('"') else {
            return err("a string is not closed");
        };
        let inner = self.rest().get(..n).unwrap_or_default().to_owned();
        self.i += n + 1;
        let mut sub = P {
            s: &inner,
            i: 0,
            depth: self.depth,
        };
        let (nodes, _) = sub.block()?;
        Ok(Expr::Str(nodes))
    }

    fn number(&mut self) -> R<Expr> {
        let text: String = self
            .rest()
            .chars()
            .enumerate()
            .take_while(|(i, c)| c.is_ascii_digit() || (*c == '-' && *i == 0) || *c == '.')
            .map(|(_, c)| c)
            .collect();
        // `1..3` is a range, not a number with two points.
        let text = match text.find("..") {
            Some(n) => text.get(..n).unwrap_or_default().to_owned(),
            None => text,
        };
        self.i += text.len();
        let v = text
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| text.parse::<f64>().map(Value::from));
        v.map(Expr::Lit)
            .map_err(|_| Error(format!("{text} is not a number")))
    }

    fn list(&mut self) -> R<Expr> {
        self.i += 1;
        self.ws();
        if self.eat("]") {
            return Ok(Expr::List(Vec::new()));
        }
        let first = self.expr()?;
        self.ws();
        if self.eat("..") {
            let last = self.expr()?;
            self.ws();
            if !self.eat("]") {
                return err("a range's ] is missing");
            }
            return Ok(Expr::Range(Box::new(first), Box::new(last)));
        }
        let mut items = vec![first];
        if self.eat(",") {
            items.extend(self.args(']')?);
        } else if !self.eat("]") {
            return err("a list's ] is missing");
        }
        Ok(Expr::List(items))
    }

    fn map(&mut self) -> R<Expr> {
        self.i += 1;
        let mut pairs = Vec::new();
        loop {
            self.ws();
            if self.eat("}") {
                return Ok(Expr::Map(pairs));
            }
            let k = self.expr()?;
            self.ws();
            if !self.eat(":") {
                return err("a map entry's : is missing");
            }
            let v = self.expr()?;
            pairs.push((k, v));
            self.ws();
            self.eat(",");
        }
    }
}
