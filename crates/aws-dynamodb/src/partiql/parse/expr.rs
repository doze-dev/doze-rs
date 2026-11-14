use super::ast::{Cmp, Error, Expr, Func, Lit, Operand, syntax};
use super::lex::{Tok, describe};

impl super::Parser {
    pub(super) fn expr(&mut self) -> Result<Expr, Error> {
        self.deeper()?;
        let mut left = self.and()?;
        while self.eat_word("OR") {
            let right = self.and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, Error> {
        let mut left = self.not()?;
        while self.eat_word("AND") {
            let right = self.not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, Error> {
        if self.eat_word("NOT") {
            self.deeper()?;
            let inner = self.not()?;
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        if self.eat_sym("(") {
            let inner = self.expr()?;
            self.sym(")")?;
            return Ok(inner);
        }
        if self.is_word("EXISTS") {
            return Err(Error::NotBuilt("EXISTS"));
        }
        if let Some(f) = self.condition_function()? {
            self.at += 2;
            let p = self.path()?;
            self.sym(",")?;
            let v = self.operand()?;
            self.sym(")")?;
            return Ok(Expr::Func(f, p, v));
        }
        let left = self.operand()?;
        if self.eat_word("BETWEEN") {
            let lo = self.operand()?;
            self.word("AND")?;
            let hi = self.operand()?;
            return Ok(Expr::Between(left, lo, hi));
        }
        if self.eat_word("IN") {
            return Ok(Expr::In(left, self.in_list()?));
        }
        if self.eat_word("IS") {
            return self.is(left);
        }
        let op = match self.next() {
            Some(Tok::Sym("=")) => Cmp::Eq,
            Some(Tok::Sym("<>" | "!=")) => Cmp::Ne,
            Some(Tok::Sym("<")) => Cmp::Lt,
            Some(Tok::Sym("<=")) => Cmp::Le,
            Some(Tok::Sym(">")) => Cmp::Gt,
            Some(Tok::Sym(">=")) => Cmp::Ge,
            Some(t) => return syntax(format!("expected a comparison, found {}", describe(&t))),
            None => return syntax("expected a comparison at the end of the statement"),
        };
        let right = self.operand()?;
        Ok(Expr::Cmp(left, op, right))
    }

    /// The condition function the next tokens call, if they call one (`size` is an operand, not a condition).
    fn condition_function(&self) -> Result<Option<Func>, Error> {
        let Some(Tok::Word(w)) = self.peek() else {
            return Ok(None);
        };
        if !matches!(self.peek_at(1), Some(Tok::Sym("("))) {
            return Ok(None);
        }
        match w.to_ascii_lowercase().as_str() {
            "begins_with" => Ok(Some(Func::BeginsWith)),
            "contains" => Ok(Some(Func::Contains)),
            "attribute_type" => Ok(Some(Func::AttributeType)),
            "size" => Ok(None),
            other => syntax(format!("{other} is not a condition function")),
        }
    }

    /// `[a, b]` or `(a, b)`, after `IN`.
    fn in_list(&mut self) -> Result<Vec<Operand>, Error> {
        let close = if self.eat_sym("[") {
            "]"
        } else if self.eat_sym("(") {
            ")"
        } else {
            return self.expected("'[' or '('");
        };
        let mut list = vec![self.operand()?];
        while self.eat_sym(",") {
            list.push(self.operand()?);
            if list.len() > 100 {
                return syntax("IN takes at most 100 values");
            }
        }
        self.sym(close)?;
        Ok(list)
    }

    /// `IS [NOT] MISSING` and `IS [NOT] NULL`, after `IS`.
    fn is(&mut self, left: Operand) -> Result<Expr, Error> {
        let negated = self.eat_word("NOT");
        let Operand::Path(p) = left else {
            return syntax("IS tests an attribute");
        };
        if self.eat_word("MISSING") {
            return Ok(Expr::Missing(p, !negated));
        }
        if self.eat_word("NULL") {
            return Ok(Expr::Null(p, !negated));
        }
        self.expected("MISSING or NULL")
    }

    pub(super) fn operand(&mut self) -> Result<Operand, Error> {
        match self.peek() {
            Some(Tok::Word(w))
                if w.eq_ignore_ascii_case("size")
                    && matches!(self.peek_at(1), Some(Tok::Sym("("))) =>
            {
                self.at += 2;
                let p = self.path()?;
                self.sym(")")?;
                Ok(Operand::Size(p))
            }
            Some(Tok::Word(w))
                if ["true", "false", "null", "missing"]
                    .iter()
                    .any(|k| w.eq_ignore_ascii_case(k)) =>
            {
                Ok(Operand::Value(self.literal()?))
            }
            Some(Tok::Word(_) | Tok::Quoted(_)) => Ok(Operand::Path(self.path()?)),
            _ => Ok(Operand::Value(self.literal()?)),
        }
    }

    pub(super) fn literal(&mut self) -> Result<Lit, Error> {
        let Some(t) = self.next() else {
            return syntax("expected a value at the end of the statement");
        };
        match t {
            Tok::Param => {
                self.params += 1;
                Ok(Lit::Param(self.params - 1))
            }
            Tok::Str(s) => Ok(Lit::Str(s)),
            Tok::Num(n) => Ok(Lit::Num(n)),
            Tok::Sym("-") => match self.next() {
                Some(Tok::Num(n)) => Ok(Lit::Num(format!("-{n}"))),
                _ => syntax("'-' must be followed by a number"),
            },
            Tok::Word(w) if w.eq_ignore_ascii_case("true") => Ok(Lit::Bool(true)),
            Tok::Word(w) if w.eq_ignore_ascii_case("false") => Ok(Lit::Bool(false)),
            Tok::Word(w) if w.eq_ignore_ascii_case("null") => Ok(Lit::Null),
            Tok::Word(w) if w.eq_ignore_ascii_case("missing") => {
                Err(Error::NotBuilt("MISSING as a value"))
            }
            Tok::Sym("[") => {
                self.deeper()?;
                let items = self.items("]")?;
                self.depth -= 1;
                Ok(Lit::List(items))
            }
            Tok::Sym("<<") => {
                self.deeper()?;
                let items = self.items(">>")?;
                self.depth -= 1;
                if items.is_empty() {
                    return syntax("a set cannot be empty");
                }
                Ok(Lit::Bag(items))
            }
            Tok::Sym("{") => {
                self.deeper()?;
                let mut fields = Vec::new();
                if !self.eat_sym("}") {
                    loop {
                        let key = match self.next() {
                            Some(Tok::Str(k)) => k,
                            _ => return syntax("a map key is a quoted text: {'key': value}"),
                        };
                        self.sym(":")?;
                        fields.push((key, self.literal()?));
                        if self.eat_sym("}") {
                            break;
                        }
                        self.sym(",")?;
                    }
                }
                self.depth -= 1;
                Ok(Lit::Map(fields))
            }
            other => syntax(format!("expected a value, found {}", describe(&other))),
        }
    }

    fn items(&mut self, close: &str) -> Result<Vec<Lit>, Error> {
        let mut out = Vec::new();
        if self.eat_sym(close) {
            return Ok(out);
        }
        loop {
            out.push(self.literal()?);
            if self.eat_sym(close) {
                return Ok(out);
            }
            self.sym(",")?;
        }
    }
}
